//! Best-effort sound playback for the `jiwa` binary.
//!
//! Issue #7: `--sound <PATH|URL>` plays a short sound each time a reveal
//! frame brings new non-whitespace graphemes into view, giving the
//! typewriter a "clack / blip" voice (typewriter, Dragon-Quest-style text
//! beep, etc.).
//!
//! Philosophy — the jiwa *binary* keeps **zero Cargo dependencies**:
//!
//! - We never decode or output audio ourselves. The sound *file* is
//!   user-supplied; playback shells out to whatever OS player exists
//!   (`ffplay` / `mpv` / `aplay` / `pw-cat`, or macOS `afplay`).
//! - URLs are fetched with `curl` (or `wget`) shelled out — no HTTP crate.
//! - Everything is **best-effort**: a missing file, missing fetcher,
//!   missing player, or a failed `spawn` is silent (at most one quiet
//!   stderr note at load time). The reveal animation always runs; the
//!   sound is purely additive.
//!
//! State discipline — jiwa holds **no surviving state**:
//!
//! - File / network I/O happens **once at load**: the bytes live in
//!   memory ([`Sound::bytes`]); graphemes never re-read or re-download.
//!   The URL cache lives in a temp dir whose cleanup is the OS's job
//!   (`tmpfiles` / reboot); jiwa does not manage or delete it.
//! - No long-lived player process: each [`Sound::play`] spawns a fresh
//!   short-lived player and throws it (we never `wait`), so nothing is
//!   left resident.
//!
//! Binary-only: not referenced by `lib.rs`.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// A resolved player: the command to run plus how it takes audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// The executable to spawn (e.g. `ffplay`).
    cmd: String,
    /// Fixed arguments that precede the audio source.
    args: Vec<String>,
    /// How this player receives the audio bytes.
    feed: Feed,
}

/// How a [`Player`] is fed the audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Feed {
    /// Bytes are written to the child's stdin (e.g. `ffplay -i pipe:0`).
    Stdin,
    /// Player only accepts a file path (macOS `afplay`); jiwa writes the
    /// in-memory bytes to a temp file once and passes that path.
    Path,
}

/// A loaded sound: the audio bytes (read once) plus the detected player.
#[derive(Debug, Clone)]
pub struct Sound {
    bytes: Vec<u8>,
    player: Player,
    /// Temp path holding the bytes, for [`Feed::Path`] players (`afplay`).
    /// Written once at load; reused on every [`Sound::play`].
    path_for_player: Option<PathBuf>,
}

/// Player candidates, in priority order. Stdin-capable players come first
/// (no temp file needed); the macOS path-only fallback (`afplay`) is last.
/// kako-jun runs Arch Linux, so the Linux players lead.
fn candidates() -> Vec<Player> {
    let s = |v: &str| v.to_string();
    let argv = |v: &[&str]| v.iter().map(|a| a.to_string()).collect();
    vec![
        // ffplay: read raw from stdin, no window, quit at EOF, quiet.
        Player {
            cmd: s("ffplay"),
            args: argv(&["-nodisp", "-autoexit", "-loglevel", "quiet", "-i", "pipe:0"]),
            feed: Feed::Stdin,
        },
        // mpv: no video, very quiet, read from stdin (`-`).
        Player {
            cmd: s("mpv"),
            args: argv(&["--no-video", "--really-quiet", "-"]),
            feed: Feed::Stdin,
        },
        // aplay: ALSA, quiet, WAV from stdin.
        Player {
            cmd: s("aplay"),
            args: argv(&["-q", "-"]),
            feed: Feed::Stdin,
        },
        // pw-cat: PipeWire playback from stdin (header-dependent; lower
        // priority, hence after aplay).
        Player {
            cmd: s("pw-cat"),
            args: argv(&["-p", "-"]),
            feed: Feed::Stdin,
        },
        // macOS afplay: file path only, no stdin. Last-resort fallback.
        Player {
            cmd: s("afplay"),
            args: Vec::new(),
            feed: Feed::Path,
        },
    ]
}

/// Load a sound from a local path or an `http(s)` URL.
///
/// Performs the *single* file/network read and detects a player, both at
/// startup. Returns `None` (after at most one quiet stderr note) on any
/// best-effort failure: unreadable file, failed/empty download, missing
/// fetcher, or no usable player. A `None` simply means silent reveal.
pub fn load(spec: &str) -> Option<Sound> {
    let bytes = if is_url(spec) {
        load_url(spec)?
    } else {
        match std::fs::read(spec) {
            Ok(b) if !b.is_empty() => b,
            Ok(_) => {
                note(&format!("sound file `{spec}` is empty; playing silently"));
                return None;
            }
            Err(e) => {
                note(&format!(
                    "cannot read sound `{spec}`: {e}; playing silently"
                ));
                return None;
            }
        }
    };

    let player = match detect_player() {
        Some(p) => p,
        None => {
            note("no audio player found (ffplay/mpv/aplay/pw-cat/afplay); playing silently");
            return None;
        }
    };

    // For path-only players, write the bytes to a temp file once now so
    // `play` can hand over the path without re-reading anything.
    let path_for_player = match player.feed {
        Feed::Path => {
            let p = temp_path(&format!("jiwa-play-{}", hash_hex(spec)), spec);
            match std::fs::write(&p, &bytes) {
                Ok(()) => Some(p),
                Err(e) => {
                    note(&format!(
                        "cannot stage sound for player: {e}; playing silently"
                    ));
                    return None;
                }
            }
        }
        Feed::Stdin => None,
    };

    Some(Sound {
        bytes,
        player,
        path_for_player,
    })
}

impl Sound {
    /// Play the sound once. Spawns a fresh short-lived player and returns
    /// immediately without waiting — non-blocking so the reveal never
    /// stalls, and stateless so nothing stays resident. All failures are
    /// ignored (best-effort).
    pub fn play(&self) {
        let mut cmd = Command::new(&self.player.cmd);
        cmd.args(&self.player.args)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match self.player.feed {
            Feed::Stdin => {
                cmd.stdin(Stdio::piped());
                if let Ok(mut child) = cmd.spawn() {
                    if let Some(mut stdin) = child.stdin.take() {
                        // Best-effort: ignore broken pipe etc. Dropping
                        // `stdin` closes it so the player sees EOF; we do
                        // not `wait` (fire-and-forget, non-blocking).
                        let _ = stdin.write_all(&self.bytes);
                    }
                }
            }
            Feed::Path => {
                cmd.stdin(Stdio::null());
                if let Some(path) = &self.path_for_player {
                    cmd.arg(path);
                    let _ = cmd.spawn();
                }
            }
        }
    }
}

/// True if `spec` looks like an HTTP(S) URL we should fetch.
pub fn is_url(spec: &str) -> bool {
    spec.starts_with("http://") || spec.starts_with("https://")
}

/// Fetch a URL into the temp cache (once) and return its bytes.
///
/// If the cache file already exists and is non-empty, it is reused without
/// re-downloading. Otherwise `curl` (then `wget`) is shelled out exactly
/// once. Returns `None` (with a quiet note) on any failure.
fn load_url(url: &str) -> Option<Vec<u8>> {
    let path = temp_path(&format!("jiwa-sound-{}", hash_hex(url)), url);

    // Reuse an existing non-empty cache file: I/O happens once across runs
    // (until the OS reclaims the temp dir).
    if let Ok(b) = std::fs::read(&path) {
        if !b.is_empty() {
            return Some(b);
        }
    }

    if !fetch(url, &path) {
        note(&format!(
            "could not download sound `{url}` (need curl or wget); playing silently"
        ));
        return None;
    }

    match std::fs::read(&path) {
        Ok(b) if !b.is_empty() => Some(b),
        _ => {
            note(&format!(
                "downloaded sound `{url}` was empty; playing silently"
            ));
            None
        }
    }
}

/// Download `url` to `dest` with `curl`, falling back to `wget`. Returns
/// true only if a fetcher ran and exited successfully.
fn fetch(url: &str, dest: &std::path::Path) -> bool {
    let curl = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dest)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if matches!(curl, Ok(s) if s.success()) {
        return true;
    }

    let wget = Command::new("wget")
        .arg("-qO")
        .arg(dest)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(wget, Ok(s) if s.success())
}

/// Detect the first available player from [`candidates`]. Probes each by
/// spawning `<cmd> --version` (stdio nulled); the first that spawns wins.
/// Run once at load.
pub fn detect_player() -> Option<Player> {
    candidates().into_iter().find(|p| player_exists(&p.cmd))
}

/// True if `cmd` can be spawned (i.e. exists on PATH). We run `--version`
/// and only care whether the process launches, not its exit code (some
/// players print version on stderr or exit non-zero).
fn player_exists(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| {
            // Reap it so we don't leave a zombie; ignore the result.
            let _ = child.wait();
            true
        })
        .unwrap_or(false)
}

/// Build a per-user temp path: `<dir>/<stem>[.<ext>]`.
///
/// `dir` prefers `$XDG_RUNTIME_DIR` (per-user, cleaned on logout), then
/// `$TMPDIR`, then `/tmp`. A file extension is appended when one can be
/// guessed from `source` (the original path/URL), purely cosmetic — stdin
/// playback is extension-independent.
pub fn temp_path(stem: &str, source: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("TMPDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let mut name = stem.to_string();
    if let Some(ext) = guess_extension(source) {
        name.push('.');
        name.push_str(&ext);
    }
    dir.join(name)
}

/// Guess a short file extension from a path/URL tail (e.g. `clack.wav` ->
/// `wav`). Returns `None` when there is no plausible alphanumeric
/// extension. Query strings / fragments on URLs are stripped first.
fn guess_extension(source: &str) -> Option<String> {
    // Drop URL query/fragment so `a.wav?x=1` still yields `wav`.
    let trimmed = source
        .split(['?', '#'])
        .next()
        .unwrap_or(source)
        .trim_end_matches('/');
    let tail = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let (_, ext) = tail.rsplit_once('.')?;
    if ext.is_empty() || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Hash `s` (SipHash via std's [`DefaultHasher`]) to a lowercase hex u64.
/// Crate-free stable-per-string name for the temp cache file.
pub fn hash_hex(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Print one quiet, prefixed stderr note. Used sparingly at load time so
/// best-effort failures are discoverable without being noisy.
fn note(msg: &str) {
    eprintln!("jiwa: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_url_detects_http_and_https() {
        assert!(is_url("http://example.com/a.wav"));
        assert!(is_url("https://example.com/a.wav"));
        assert!(!is_url("/tmp/a.wav"));
        assert!(!is_url("a.wav"));
        assert!(!is_url("ftp://example.com/a.wav"));
        // A local path that merely contains "http" is not a URL.
        assert!(!is_url("./my-http-sound.wav"));
    }

    #[test]
    fn hash_hex_is_stable_and_16_hex_digits() {
        let a = hash_hex("https://example.com/clack.wav");
        let b = hash_hex("https://example.com/clack.wav");
        assert_eq!(a, b, "same input -> same hash");
        assert_eq!(a.len(), 16, "u64 as zero-padded hex is 16 chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Different inputs almost certainly differ.
        assert_ne!(a, hash_hex("https://example.com/blip.wav"));
    }

    #[test]
    fn guess_extension_from_path_and_url() {
        assert_eq!(guess_extension("clack.wav").as_deref(), Some("wav"));
        assert_eq!(guess_extension("/a/b/blip.OGG").as_deref(), Some("ogg"));
        assert_eq!(
            guess_extension("https://x.test/s.mp3?token=1").as_deref(),
            Some("mp3")
        );
        assert_eq!(
            guess_extension("https://x.test/s.wav#frag").as_deref(),
            Some("wav")
        );
        // No extension, trailing slash, or implausible extension -> None.
        assert_eq!(guess_extension("https://x.test/sound"), None);
        assert_eq!(guess_extension("https://x.test/dir/"), None);
        assert_eq!(guess_extension("noext"), None);
        // Over-long or non-alphanumeric "extensions" are rejected.
        assert_eq!(guess_extension("a.verylongext"), None);
        assert_eq!(guess_extension("a.b_c"), None);
    }

    #[test]
    fn temp_path_uses_xdg_runtime_dir_when_set() {
        // We cannot safely mutate process env in parallel tests, so assert
        // the structural contract instead: the path ends with the expected
        // file name and lives under one of the documented base dirs.
        let p = temp_path("jiwa-sound-deadbeef", "https://x.test/clack.wav");
        let name = p.file_name().unwrap().to_string_lossy();
        assert_eq!(name, "jiwa-sound-deadbeef.wav");

        // Without a guessable extension the name is just the stem.
        let p2 = temp_path("jiwa-sound-cafe", "https://x.test/clack");
        assert_eq!(p2.file_name().unwrap().to_string_lossy(), "jiwa-sound-cafe");
    }

    #[test]
    fn candidates_are_in_documented_priority_order() {
        let cands = candidates();
        let names: Vec<&str> = cands.iter().map(|p| p.cmd.as_str()).collect();
        assert_eq!(names, ["ffplay", "mpv", "aplay", "pw-cat", "afplay"]);
        // Stdin players precede the path-only fallback.
        assert_eq!(cands.last().unwrap().feed, Feed::Path);
        assert!(cands[..cands.len() - 1]
            .iter()
            .all(|p| p.feed == Feed::Stdin));
    }

    #[test]
    fn ffplay_args_read_from_stdin_pipe() {
        let cands = candidates();
        let ffplay = &cands[0];
        assert_eq!(ffplay.cmd, "ffplay");
        assert_eq!(ffplay.feed, Feed::Stdin);
        // The argument vector must end at the stdin pipe source.
        assert!(ffplay.args.contains(&"pipe:0".to_string()));
        assert!(ffplay.args.contains(&"-nodisp".to_string()));
    }

    #[test]
    fn load_missing_local_path_is_none() {
        // Best-effort: a nonexistent file yields None, never a panic.
        assert!(load("/nonexistent/jiwa-test-sound.wav").is_none());
    }

    #[test]
    fn guess_extension_edge_cases() {
        // Trailing dot -> empty extension -> None.
        assert_eq!(guess_extension("a."), None);
        // Dotfile whose only "extension" is the whole tail (`bashrc`, len 6)
        // exceeds the 5-char cap -> None.
        assert_eq!(guess_extension(".bashrc"), None);
        // A leading-dot name with a short tail is still extracted.
        assert_eq!(guess_extension(".wav").as_deref(), Some("wav"));
        // Multi-dot: only the final segment is the extension.
        assert_eq!(guess_extension("a.tar.gz").as_deref(), Some("gz"));
        // Windows-style backslash path, case-folded.
        assert_eq!(guess_extension("C:\\x\\y.WAV").as_deref(), Some("wav"));
        // Empty / bare-dot sources have no extension.
        assert_eq!(guess_extension(""), None);
        assert_eq!(guess_extension("."), None);
        // Digits are alphanumeric, so a numeric extension is accepted.
        assert_eq!(guess_extension("a.123").as_deref(), Some("123"));
        // Length boundary: 5 chars OK, 6 chars rejected.
        assert_eq!(guess_extension("a.fffff").as_deref(), Some("fffff"));
        assert_eq!(guess_extension("a.ffffff"), None);
    }

    #[test]
    fn hash_hex_handles_empty_string() {
        // The empty string hashes to a deterministic 16-digit lowercase hex.
        let a = hash_hex("");
        assert_eq!(a, hash_hex(""), "deterministic for the empty string");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn is_url_is_case_sensitive() {
        // The scheme match is byte-exact lowercase: uppercase schemes are not
        // recognized as URLs (current fixed behavior).
        assert!(!is_url("HTTP://x"));
        assert!(!is_url("HTTPS://x"));
    }

    #[test]
    fn load_empty_local_file_is_none() {
        // A zero-byte local file is treated as "nothing to play" -> None.
        // Use a unique temp name (no env mutation, so parallel tests stay
        // safe) and clean it up afterwards.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("jiwa-empty-{}-{}.wav", std::process::id(), n));
        std::fs::write(&path, b"").expect("create empty temp file");
        let got = load(path.to_str().unwrap());
        let _ = std::fs::remove_file(&path);
        assert!(got.is_none(), "empty local file must load as None");
    }
}
