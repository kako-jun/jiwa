//! Integration tests for the `jiwa` binary.
//!
//! These drive the real compiled binary via `CARGO_BIN_EXE_jiwa`, piping
//! stdin in and capturing stdout/stderr. Because the child's stdout is a
//! pipe (not a TTY), `jiwa` takes its pass-through path: input is echoed
//! verbatim (with a guaranteed trailing newline) and no cursor-control or
//! frame-erase escapes are emitted. We assert exactly that contract, plus
//! the `--help`/`--version` and argument-error exit codes.

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// Run the binary with `args`, feeding `stdin` (bytes), and capture output.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jiwa"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn jiwa");
    {
        let mut child_stdin = child.stdin.take().expect("child stdin");
        // A child that rejects its arguments (exit 2) can close stdin before
        // we finish writing, yielding a BrokenPipe; that is expected for the
        // parse-error tests, so tolerate it rather than panicking.
        match child_stdin.write_all(stdin) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => panic!("write stdin: {e:?}"),
        }
        // child_stdin dropped here -> EOF for the child.
    }
    child.wait_with_output().expect("wait for jiwa")
}

/// True if `haystack` contains the byte subsequence `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Assert no animation/cursor-control noise leaked into a pass-through stream.
fn assert_no_ansi_noise(stdout: &[u8]) {
    // Hide-cursor, show-cursor, autowrap off/on, line-erase, cursor-up.
    assert!(!contains(stdout, b"\x1b[?25l"), "hide-cursor leaked");
    assert!(!contains(stdout, b"\x1b[?25h"), "show-cursor leaked");
    assert!(!contains(stdout, b"\x1b[?7l"), "autowrap-off leaked");
    assert!(!contains(stdout, b"\x1b[J"), "erase-display leaked");
    assert!(!contains(stdout, b"38;2"), "jiwa foreground leaked");
}

#[test]
fn passthrough_verbatim_no_ansi_noise() {
    let out = run(&[], b"hello world");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"hello world\n");
}

#[test]
fn passthrough_appends_trailing_newline() {
    let out = run(&[], b"no newline");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.ends_with(b"\n"));
    assert_eq!(out.stdout, b"no newline\n");
}

#[test]
fn passthrough_preserves_existing_trailing_newline() {
    // Already newline-terminated input must not be doubled.
    let out = run(&[], b"line\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"line\n");
}

#[test]
fn passthrough_empty_stdin() {
    let out = run(&[], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"\n");
}

#[test]
fn passthrough_preserves_utf8_and_input_ansi() {
    // Multibyte text plus pre-existing color escapes survive byte-for-byte.
    let input = "\x1b[31m世界\x1b[0m".as_bytes();
    let out = run(&[], input);
    assert_eq!(out.status.code(), Some(0));
    let mut expected = input.to_vec();
    expected.push(b'\n');
    assert_eq!(out.stdout, expected);
}

#[test]
fn no_animation_flags_passes_through_even_with_colors() {
    // Color flags alone (no fade/stagger) do not enable animation.
    let out = run(&["--from", "#000", "--to", "#fff"], b"plain");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"plain\n");
}

#[test]
fn fade_zero_stagger_zero_explicit_passthrough() {
    // Explicit zero durations still mean "no animation".
    let out = run(&["--fade", "0", "--stagger", "0"], b"plain");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"plain\n");
}

#[test]
fn help_exits_zero_and_prints_usage() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag], b"");
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert!(contains(&out.stdout, b"USAGE"), "{flag} usage missing");
    }
}

#[test]
fn version_exits_zero() {
    for flag in ["-V", "--version"] {
        let out = run(&[flag], b"");
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert!(contains(&out.stdout, b"jiwa "), "{flag} version missing");
    }
}

#[test]
fn unknown_flag_exits_two() {
    let out = run(&["--nope"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty(), "expected an error message");
}

#[test]
fn invalid_value_exits_two() {
    let bad_fade = run(&["--fade", "bad"], b"");
    assert_eq!(bad_fade.status.code(), Some(2));
    let bad_color = run(&["--from", "#xyz"], b"");
    assert_eq!(bad_color.status.code(), Some(2));
}

#[test]
fn missing_value_exits_two() {
    // Trailing value-taking flag with nothing after it.
    let out = run(&["--fade"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn escape_only_input_with_animation_passes_through() {
    // Input with no printable graphemes (escape sequences only) must not be
    // swallowed even when animation is requested: it falls back to verbatim
    // pass-through (here over a non-TTY pipe, with a trailing newline added).
    let input = "\x1b[31m\x1b[0m".as_bytes();
    let out = run(&["--fade", "200ms"], input);
    assert_eq!(out.status.code(), Some(0));
    let mut expected = input.to_vec();
    expected.push(b'\n');
    assert_eq!(out.stdout, expected);
}

#[test]
fn equals_form_flags_accepted() {
    // `--flag=value` is accepted by the real binary (exit 0, clean pipe).
    let out = run(&["--fade=200ms", "--stagger=30ms"], b"streamed");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"streamed\n");
}

#[test]
fn pipe_into_pipe_stays_clean() {
    // Even with animation requested, a non-TTY stdout forces pass-through:
    // verbatim body, no control codes.
    let out = run(&["--fade", "200ms"], b"streamed");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"streamed\n");
}

// --- Reader mode (`--read`) ---
//
// PTY gap: these integration tests only exercise the non-TTY pass-through
// path and the parse-error path. `cargo test` always gives the child a pipe
// for stdout (never a TTY), so the interactive loop in `run_reader` —
// sending Enter, erasing the prompt, the q/EOF/error early-exit branches,
// and the `TermGuard` restore — is never reached here. That loop needs a
// `/dev/tty` + PTY harness and is verified manually; the `erase_prompt`
// byte sequences are unit-tested in `main.rs`.

#[test]
fn read_mode_non_tty_passes_through() {
    // Reader mode requires a TTY stdout; over a pipe it falls back to clean
    // verbatim pass-through (no prompt, no cursor noise).
    let out = run(&["--read"], b"First sentence. Second sentence.");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"First sentence. Second sentence.\n");
}

#[test]
fn read_mode_paragraph_non_tty_passes_through() {
    // Same pass-through guarantee with an explicit `--by paragraph`.
    let out = run(&["--read", "--by", "paragraph"], b"Para one.\n\nPara two.");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"Para one.\n\nPara two.\n");
}

#[test]
fn read_mode_invalid_unit_exits_two() {
    // An unknown `--by` unit is rejected at parse time (exit 2), even under
    // `--read`.
    let out = run(&["--read", "--by", "word"], b"text");
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty(), "expected an error message");
}

#[test]
fn read_inline_value_rejected() {
    // `--read` is value-less; an inline `=foo` must be rejected (exit 2).
    let out = run(&["--read=foo"], b"text");
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty(), "expected an error message");
}

#[test]
fn read_mode_empty_stdin() {
    // Empty stdin under reader mode still produces clean output: a lone
    // trailing newline, matching the passthrough contract.
    let out = run(&["--read"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"\n");
}

// --- `--sound` (Issue #7) ---
//
// Over a non-TTY pipe the reveal never animates, so the sound is never
// loaded (no file/network I/O, no player spawn): we assert the passthrough
// stays byte-clean and quiet. Real playback, downloads, and player
// detection are environment-dependent and intentionally not auto-tested.

#[test]
fn sound_missing_path_passthrough_clean() {
    // A nonexistent sound path is irrelevant on the non-TTY path: passthrough
    // never reaches `sound::load`, so there is no I/O and no stderr note.
    let out = run(&["--sound", "/nonexistent.wav"], b"hello");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"hello\n");
    assert!(out.stderr.is_empty(), "passthrough must not load the sound");
}

#[test]
fn sound_url_value_passthrough_clean() {
    // A URL sound value likewise triggers no download on the passthrough path.
    let out = run(&["--sound=https://x.test/blip.wav"], b"hello");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"hello\n");
    assert!(out.stderr.is_empty(), "passthrough must not fetch the URL");
}

#[test]
fn sound_missing_value_exits_two() {
    // `--sound` is value-taking; a trailing flag with no value errors.
    let out = run(&["--sound"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty(), "expected an error message");
}

#[test]
fn sound_empty_value_exits_two() {
    // An empty value is rejected at parse time (both the `=` and the
    // separate-argument forms).
    let eq = run(&["--sound="], b"");
    assert_eq!(eq.status.code(), Some(2));
    assert!(!eq.stderr.is_empty(), "expected an error message");

    let sep = run(&["--sound", ""], b"");
    assert_eq!(sep.status.code(), Some(2));
    assert!(!sep.stderr.is_empty(), "expected an error message");
}

#[test]
fn read_with_sound_passthrough_clean() {
    // Reader mode + `--sound` over a non-TTY pipe stays verbatim and quiet:
    // no prompt, no cursor noise, no sound load.
    let out = run(&["--read", "--sound", "/nonexistent.wav"], b"hello");
    assert_eq!(out.status.code(), Some(0));
    assert_no_ansi_noise(&out.stdout);
    assert_eq!(out.stdout, b"hello\n");
    assert!(out.stderr.is_empty(), "passthrough must not load the sound");
}
