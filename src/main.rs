//! `jiwa` CLI — pipe text in, watch it reveal.
//!
//! Dependency-free terminal control: we write raw ANSI escapes straight
//! to stdout and detect a TTY with `std::io::IsTerminal`. The reveal
//! engine itself comes from the `jiwa` library crate — this binary never
//! reimplements timing or fading.
//!
//! Pipe-safety: when stdout is not a terminal (or no animation was
//! requested) we pass the input through verbatim so downstream tools and
//! files never receive cursor-control noise.

mod cli;
mod render;

use std::io::{self, IsTerminal, Read, Write};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use jiwa::{RevealHandle, RevealOpts, Rgb};

use cli::{Action, CliOpts};
use render::{plain_text, render_frame, tokenize, visible_newline_rows};

fn main() -> ExitCode {
    let args = std::env::args().skip(1);
    let action = match cli::parse_args(args) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    let opts = match action {
        Action::Help => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Action::Version => {
            println!("jiwa {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Action::Run(opts) => opts,
    };

    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("jiwa: failed to read stdin: {e}");
        return ExitCode::from(2);
    }

    let animate = !(opts.fade.is_zero() && opts.stagger.is_zero());
    let is_tty = io::stdout().is_terminal();

    if !animate || !is_tty {
        passthrough(&input);
        return ExitCode::SUCCESS;
    }

    animate_reveal(&input, &opts);
    ExitCode::SUCCESS
}

/// Emit the input verbatim, ensuring a trailing newline. Used for the
/// non-TTY / no-animation paths so pipes and redirects stay clean.
fn passthrough(input: &str) {
    let mut out = io::stdout().lock();
    let _ = out.write_all(input.as_bytes());
    if !input.ends_with('\n') {
        let _ = out.write_all(b"\n");
    }
    let _ = out.flush();
}

/// Terminal control written when entering the animation: hide the cursor
/// and disable autowrap (so in-place frames clip instead of wrapping).
const ENTER: &[u8] = b"\x1b[?25l\x1b[?7l";
/// Terminal control to restore the terminal: show the cursor and re-enable
/// autowrap. Used by [`TermGuard`] so we always end clean.
const RESTORE: &[u8] = b"\x1b[?25h\x1b[?7h";
/// Re-enable autowrap on its own (used before the final confirmed render so
/// long lines wrap in scrollback). It is the tail of [`RESTORE`].
const AUTOWRAP_ON: &[u8] = b"\x1b[?7h";

/// Restores cursor visibility and line-wrap on drop — covers both normal
/// return and panic-unwinding so the terminal is never left broken.
struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        let _ = out.write_all(RESTORE);
        let _ = out.flush();
    }
}

/// Build the byte sequence that erases the previous in-place frame.
///
/// When `prev_rows > 0` we walk the cursor back up that many rows
/// (`\x1b[{n}A`); then in all cases return to column 0 and clear from the
/// cursor downward (`\r\x1b[J`). With `prev_rows == 0` only the latter runs.
fn erase_prev_frame(prev_rows: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    if prev_rows > 0 {
        buf.extend_from_slice(format!("\x1b[{prev_rows}A").as_bytes());
    }
    buf.extend_from_slice(b"\r\x1b[J");
    buf
}

/// Build the byte sequence for the final confirmed render.
///
/// Erases the last in-place frame, re-enables autowrap *before* drawing so
/// long lines wrap correctly in the permanent scrollback output, draws the
/// full text, and appends a trailing newline so the shell prompt lands on
/// its own line.
fn final_render_sequence(prev_rows: usize, final_frame: &str) -> Vec<u8> {
    let mut buf = erase_prev_frame(prev_rows);
    buf.extend_from_slice(AUTOWRAP_ON);
    buf.extend_from_slice(final_frame.as_bytes());
    buf.push(b'\n');
    buf
}

fn animate_reveal(input: &str, opts: &CliOpts) {
    let tokens = tokenize(input);
    let plain = plain_text(&tokens);

    let reveal_opts = RevealOpts {
        char_interval: opts.stagger,
        fade_duration: opts.fade,
        fade_from: opts.from,
        fade_to: opts.to,
    };

    let start = Instant::now();
    let handle = RevealHandle::start_at(&plain, reveal_opts, start);

    // No printable graphemes (e.g. escape-only input): there is nothing to
    // animate, so behave exactly like the non-TTY pass-through path and emit
    // the input verbatim rather than entering cursor control (which would
    // otherwise swallow the input).
    if handle.total_graphemes() == 0 {
        passthrough(input);
        return;
    }

    let mut out = io::stdout().lock();
    // Hide cursor + disable autowrap during the animation; the guard puts
    // both back no matter how we leave.
    let _ = out.write_all(ENTER);
    let _ = out.flush();
    let _guard = TermGuard;

    // Use the nominal fps directly so the frame interval matches it exactly
    // (e.g. 60 fps -> 16.667 ms, not the 16 ms an integer division gives).
    let frame_delay = Duration::from_secs_f64(1.0 / f64::from(opts.fps));
    let mut prev_rows = 0usize;

    // The loop yields its final snapshot, making that fully-revealed frame
    // the single source of truth for the confirmed render's colors. (The
    // loop always runs at least once and breaks only once `is_done`.)
    let snap = loop {
        let now = Instant::now();

        let _ = out.write_all(&erase_prev_frame(prev_rows));

        let snap = handle.snapshot(now);
        let visible = snap.len();
        let colors: Vec<Rgb> = snap.iter().map(|g| g.color).collect();

        let frame = render_frame(&tokens.tokens, visible, &colors, tokens.has_input_color);
        let _ = out.write_all(frame.as_bytes());
        prev_rows = visible_newline_rows(&tokens.tokens, visible);
        let _ = out.flush();

        if handle.is_done(now) {
            break snap;
        }
        thread::sleep(frame_delay);
    };

    // Final confirmed render that stays in scrollback. `is_done` was true,
    // so `snap` holds every grapheme at its fade-complete color — reuse it
    // rather than reconstructing the colors independently.
    let visible = snap.len();
    let colors: Vec<Rgb> = snap.iter().map(|g| g.color).collect();
    let final_frame = render_frame(&tokens.tokens, visible, &colors, tokens.has_input_color);
    let _ = out.write_all(&final_render_sequence(prev_rows, &final_frame));
    let _ = out.flush();
    // `_guard` drops here, restoring cursor visibility.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erase_prev_frame_zero_rows_clears_in_place() {
        // With no prior rows, only column-return + erase-down is emitted
        // (no cursor-up sequence).
        assert_eq!(erase_prev_frame(0), b"\r\x1b[J".to_vec());
    }

    #[test]
    fn erase_prev_frame_walks_up_then_clears() {
        // With prior rows, cursor-up comes first, then column-return + erase.
        assert_eq!(erase_prev_frame(3), b"\x1b[3A\r\x1b[J".to_vec());
    }

    #[test]
    fn final_render_sequence_reenables_autowrap_before_frame() {
        let seq = final_render_sequence(0, "FINAL");
        assert_eq!(seq, b"\r\x1b[J\x1b[?7hFINAL\n".to_vec());

        // The autowrap-restore byte run must appear before the frame text,
        // so the permanent output wraps long lines.
        let s = String::from_utf8(seq).unwrap();
        let autowrap = std::str::from_utf8(AUTOWRAP_ON).unwrap();
        let wrap_at = s.find(autowrap).expect("autowrap present");
        let frame_at = s.find("FINAL").expect("frame present");
        assert!(wrap_at < frame_at, "autowrap must precede the final frame");
    }

    #[test]
    fn final_render_sequence_walks_up_for_prior_rows_and_ends_with_newline() {
        let seq = final_render_sequence(2, "X");
        assert_eq!(seq, b"\x1b[2A\r\x1b[J\x1b[?7hX\n".to_vec());
        assert_eq!(seq.last(), Some(&b'\n'), "must end with a trailing newline");
    }

    #[test]
    fn enter_and_restore_share_autowrap_control() {
        // ENTER disables autowrap; RESTORE re-enables it, and AUTOWRAP_ON is
        // exactly RESTORE's tail so the two stay in sync.
        assert_eq!(ENTER, b"\x1b[?25l\x1b[?7l");
        assert_eq!(RESTORE, b"\x1b[?25h\x1b[?7h");
        assert!(RESTORE.ends_with(AUTOWRAP_ON));
    }
}
