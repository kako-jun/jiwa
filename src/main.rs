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

/// Restores cursor visibility and line-wrap on drop — covers both normal
/// return and panic-unwinding so the terminal is never left broken.
struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        // Show cursor, re-enable autowrap.
        let _ = out.write_all(b"\x1b[?25h\x1b[?7h");
        let _ = out.flush();
    }
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

    let mut out = io::stdout().lock();
    // Hide cursor + disable autowrap during the animation; the guard puts
    // both back no matter how we leave.
    let _ = out.write_all(b"\x1b[?25l\x1b[?7l");
    let _ = out.flush();
    let _guard = TermGuard;

    let frame_delay = Duration::from_millis(1000 / u64::from(opts.fps));
    let mut prev_rows = 0usize;

    loop {
        let now = Instant::now();

        // Erase the previous frame: walk back up `prev_rows`, go to column
        // 0, then clear from the cursor downward.
        if prev_rows > 0 {
            let _ = write!(out, "\x1b[{prev_rows}A");
        }
        let _ = out.write_all(b"\r\x1b[J");

        let snap = handle.snapshot(now);
        let visible = snap.len();
        let colors: Vec<Rgb> = snap.iter().map(|g| g.color).collect();

        let frame = render_frame(&tokens.tokens, visible, &colors, tokens.has_input_color);
        let _ = out.write_all(frame.as_bytes());
        prev_rows = visible_newline_rows(&tokens.tokens, visible);
        let _ = out.flush();

        if handle.is_done(now) {
            break;
        }
        thread::sleep(frame_delay);
    }

    // Final confirmed render that stays in scrollback. Erase the in-place
    // frame, re-enable autowrap so long lines wrap correctly in the
    // permanent output, then draw the full text once.
    if prev_rows > 0 {
        let _ = write!(out, "\x1b[{prev_rows}A");
    }
    let _ = out.write_all(b"\r\x1b[J");
    let _ = out.write_all(b"\x1b[?7h");

    let total = handle.total_graphemes();
    let final_colors: Vec<Rgb> = (0..total).map(|_| opts.to).collect();
    let final_frame = render_frame(&tokens.tokens, total, &final_colors, tokens.has_input_color);
    let _ = out.write_all(final_frame.as_bytes());

    // Trailing newline so the shell prompt lands on its own line.
    let _ = out.write_all(b"\n");
    let _ = out.flush();
    // `_guard` drops here, restoring cursor visibility.
}
