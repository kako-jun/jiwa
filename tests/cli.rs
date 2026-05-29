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
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(stdin)
        .expect("write stdin");
    // stdin dropped here -> EOF for the child.
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
