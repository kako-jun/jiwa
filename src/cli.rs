//! Argument parsing for the `jiwa` binary.
//!
//! Deliberately dependency-free: no `clap`, no `crossterm`. We hand-roll
//! a tiny flag scanner plus two small value parsers (`parse_duration`,
//! `parse_color`) so they can be unit-tested in isolation. None of this
//! is referenced by the library crate (`lib.rs`); it is binary-only.

use std::time::Duration;

use jiwa::Rgb;

use crate::reader::Unit;

/// Fully-parsed CLI options.
#[derive(Debug, Clone, PartialEq)]
pub struct CliOpts {
    pub fade: Duration,
    pub stagger: Duration,
    pub from: Rgb,
    pub to: Rgb,
    pub fps: u32,
    /// Interactive reader (sound-novel) mode: reveal one segment at a time,
    /// waiting for Enter on `/dev/tty` between segments.
    pub read: bool,
    /// Segment unit used by reader mode.
    pub by: Unit,
}

impl Default for CliOpts {
    fn default() -> Self {
        Self {
            fade: Duration::ZERO,
            stagger: Duration::ZERO,
            from: Rgb(40, 40, 40),
            to: Rgb(220, 220, 220),
            fps: 60,
            read: false,
            by: Unit::Sentence,
        }
    }
}

/// What `main` should do after parsing.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Print usage and exit 0.
    Help,
    /// Print version and exit 0.
    Version,
    /// Run the reveal with these options.
    Run(CliOpts),
}

pub const USAGE: &str = "\
jiwa — terminal text reveal animations over a Unix pipe.

USAGE:
    jiwa [OPTIONS]

Reads all of stdin, then reveals it on stdout. With no animation flags
(or when stdout is not a TTY) the input is passed through verbatim, so
`jiwa ... | other` and `jiwa ... > file` stay clean.

OPTIONS:
    --fade <DUR>      Per-grapheme fade duration (e.g. 200ms, 1.5s, 50).
                      Default 0 (no fade). Also accepts `--fade=200ms`.
    --stagger <DUR>   Typewriter step between graphemes (e.g. 30ms).
                      Default 0 (all graphemes at once).
    --from <COLOR>    Fade start color (#rrggbb / rgb / #rgb). Default #282828.
    --to <COLOR>      Fade end color. Default #dcdcdc.
    --fps <N>         Animation frame rate, clamped to 1..=240. Default 60.
    --read            Interactive reader (sound-novel) mode: reveal one
                      segment at a time, pausing for Enter between them.
                      Reads keypresses from /dev/tty (stdin holds the text).
                      Requires a TTY stdout; otherwise the input is passed
                      through verbatim.
    --by <UNIT>       Reader segment unit: sentence (default) / paragraph /
                      line. Only meaningful with --read.
    -h, --help        Print this help and exit.
    -V, --version     Print version and exit.

Value-taking flags accept either a separate argument (`--fade 200ms`) or
an `=`-joined form (`--fade=200ms`).

READER MODE:
    `--read` turns a piped novel into a sound-novel reader: each segment
    reveals, then jiwa waits for you to press Enter before the next one.

        cat novel.txt | jiwa --read --stagger 40ms

    Press Enter to advance; `q` or Ctrl-D (EOF) ends the session. The
    waiting prompt is erased once you advance, so scrollback keeps only the
    novel text.

DURATION:
    Suffix `ms` for milliseconds, `s` for seconds (decimals allowed).
    A bare number is treated as milliseconds.

DISPLAY:
    During the animation jiwa disables line-wrap and redraws frames in
    place, so lines longer than the terminal are clipped; the final
    confirmed render re-enables wrap so long lines wrap normally in
    scrollback. If you interrupt the animation with Ctrl-C, the terminal
    may be left with the cursor hidden and wrap off (jiwa stays
    dependency-free and installs no signal handler); run `reset` to
    restore it.

COLOR:
    `#rrggbb`, `rrggbb`, `#rgb`, or `rgb`. The leading `#` is optional;
    3-digit forms expand each digit (#f0a -> #ff00aa).
";

/// Clamp range for `--fps`.
const FPS_MIN: u32 = 1;
const FPS_MAX: u32 = 240;

/// Parse argv (excluding the program name) into an [`Action`].
///
/// Returns `Err(message)` for unknown flags or invalid values; the caller
/// prints the message to stderr and exits 2.
pub fn parse_args<I, S>(args: I) -> Result<Action, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut opts = CliOpts::default();
    let mut iter = args.into_iter().peekable();

    while let Some(raw) = iter.next() {
        let arg = raw.as_ref();

        // Support the `--flag=value` form: a long flag carrying an inline
        // value. Short flags (`-h`/`-V`) and value-less flags keep the
        // separate-argument form. We split here so each match arm can ask
        // for its value uniformly via `take_value`.
        let (flag, mut inline): (&str, Option<String>) = if arg.starts_with("--") {
            match arg.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (arg, None),
            }
        } else {
            (arg, None)
        };

        match flag {
            "-h" | "--help" => {
                reject_inline(flag, &inline)?;
                return Ok(Action::Help);
            }
            "-V" | "--version" => {
                reject_inline(flag, &inline)?;
                return Ok(Action::Version);
            }
            "--fade" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                opts.fade = parse_duration(&v).map_err(|e| flag_err("--fade", &v, &e))?;
            }
            "--stagger" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                opts.stagger = parse_duration(&v).map_err(|e| flag_err("--stagger", &v, &e))?;
            }
            "--from" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                opts.from = parse_color(&v).map_err(|e| flag_err("--from", &v, &e))?;
            }
            "--to" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                opts.to = parse_color(&v).map_err(|e| flag_err("--to", &v, &e))?;
            }
            "--fps" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                let n: u32 = v
                    .trim()
                    .parse()
                    .map_err(|_| flag_err("--fps", &v, "expected an integer"))?;
                opts.fps = n.clamp(FPS_MIN, FPS_MAX);
            }
            "--read" => {
                reject_inline(flag, &inline)?;
                opts.read = true;
            }
            "--by" => {
                let v = take_value(flag, &mut inline, &mut iter)?;
                opts.by = parse_unit(&v).map_err(|e| flag_err("--by", &v, &e))?;
            }
            _ => {
                return Err(format!(
                    "jiwa: unknown argument `{arg}`\nTry `jiwa --help`."
                ));
            }
        }
    }

    Ok(Action::Run(opts))
}

/// Resolve the value for a value-taking flag.
///
/// Prefers an inline `--flag=value` value (consumed here); otherwise pulls
/// the next argument. Errors if neither is present.
fn take_value<I, S>(
    flag: &str,
    inline: &mut Option<String>,
    iter: &mut std::iter::Peekable<I>,
) -> Result<String, String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    if let Some(v) = inline.take() {
        return Ok(v);
    }
    match iter.next() {
        Some(v) => Ok(v.as_ref().to_string()),
        None => Err(format!(
            "jiwa: `{flag}` requires a value\nTry `jiwa --help`."
        )),
    }
}

fn flag_err(flag: &str, value: &str, why: &str) -> String {
    format!("jiwa: invalid value `{value}` for `{flag}`: {why}\nTry `jiwa --help`.")
}

/// Reject an inline `--flag=value` value on a flag that takes no value
/// (`--help`, `--version`). Without this, `--help=foo` would silently
/// discard the stray `=foo`.
fn reject_inline(flag: &str, inline: &Option<String>) -> Result<(), String> {
    match inline {
        Some(v) => Err(format!(
            "jiwa: `{flag}` takes no value (got `={v}`)\nTry `jiwa --help`."
        )),
        None => Ok(()),
    }
}

/// Parse a duration string.
///
/// - `200ms` -> 200 milliseconds
/// - `1s` / `1.5s` -> seconds (decimals allowed)
/// - `50` (no suffix) -> milliseconds
///
/// Negative, non-finite, or non-numeric inputs are errors.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".to_string());
    }

    let (num_str, scale_to_secs) = if let Some(rest) = s.strip_suffix("ms") {
        (rest.trim(), 1e-3_f64)
    } else if let Some(rest) = s.strip_suffix('s') {
        (rest.trim(), 1.0_f64)
    } else {
        (s, 1e-3_f64)
    };

    if num_str.is_empty() {
        return Err("missing number".to_string());
    }

    let value: f64 = num_str
        .parse()
        .map_err(|_| format!("`{num_str}` is not a number"))?;
    if !value.is_finite() || value < 0.0 {
        return Err("duration must be a non-negative finite number".to_string());
    }

    Ok(Duration::from_secs_f64(value * scale_to_secs))
}

/// Parse a hex color: `#rrggbb`, `rrggbb`, `#rgb`, or `rgb`. The leading
/// `#` is optional; 3-digit forms expand each digit (`#f0a` -> `#ff00aa`).
pub fn parse_color(s: &str) -> Result<Rgb, String> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("`{s}` is not a hex color"));
    }
    match hex.len() {
        3 => {
            let r = expand_nibble(&hex[0..1])?;
            let g = expand_nibble(&hex[1..2])?;
            let b = expand_nibble(&hex[2..3])?;
            Ok(Rgb(r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
            Ok(Rgb(r, g, b))
        }
        _ => Err(format!("`{s}` must be 3 or 6 hex digits")),
    }
}

/// Parse a reader segment unit: `sentence`, `paragraph`, or `line`.
pub fn parse_unit(s: &str) -> Result<Unit, String> {
    match s.trim() {
        "sentence" => Ok(Unit::Sentence),
        "paragraph" => Ok(Unit::Paragraph),
        "line" => Ok(Unit::Line),
        other => Err(format!(
            "`{other}` is not a unit (expected sentence, paragraph, or line)"
        )),
    }
}

fn expand_nibble(digit: &str) -> Result<u8, String> {
    let n = u8::from_str_radix(digit, 16).map_err(|e| e.to_string())?;
    // `f` (15) -> `ff` (255): n * 16 + n.
    Ok(n * 16 + n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("200ms").unwrap(), Duration::from_millis(200));
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        // Bare number is milliseconds.
        assert_eq!(parse_duration("50").unwrap(), Duration::from_millis(50));
        assert_eq!(parse_duration("0").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("-5ms").is_err());
        assert!(parse_duration("ms").is_err());
    }

    #[test]
    fn parse_color_forms() {
        assert_eq!(parse_color("#ff00aa").unwrap(), Rgb(255, 0, 170));
        assert_eq!(parse_color("ff00aa").unwrap(), Rgb(255, 0, 170));
        assert_eq!(parse_color("#f0a").unwrap(), Rgb(255, 0, 170));
        assert_eq!(parse_color("f0a").unwrap(), Rgb(255, 0, 170));
    }

    #[test]
    fn parse_color_rejects_garbage() {
        assert!(parse_color("#xyz").is_err());
        assert!(parse_color("12345").is_err());
        assert!(parse_color("").is_err());
    }

    #[test]
    fn parse_args_defaults_to_run() {
        let empty: [&str; 0] = [];
        assert_eq!(parse_args(empty).unwrap(), Action::Run(CliOpts::default()));
    }

    #[test]
    fn parse_args_help_and_version() {
        assert_eq!(parse_args(["--help"]).unwrap(), Action::Help);
        assert_eq!(parse_args(["-h"]).unwrap(), Action::Help);
        assert_eq!(parse_args(["--version"]).unwrap(), Action::Version);
        assert_eq!(parse_args(["-V"]).unwrap(), Action::Version);
    }

    #[test]
    fn parse_args_maps_flags() {
        let action = parse_args([
            "--fade",
            "200ms",
            "--stagger",
            "30ms",
            "--from",
            "#000",
            "--to",
            "#fff",
            "--fps",
            "30",
        ])
        .unwrap();
        let Action::Run(opts) = action else {
            panic!("expected Run");
        };
        assert_eq!(opts.fade, Duration::from_millis(200));
        assert_eq!(opts.stagger, Duration::from_millis(30));
        assert_eq!(opts.from, Rgb(0, 0, 0));
        assert_eq!(opts.to, Rgb(255, 255, 255));
        assert_eq!(opts.fps, 30);
    }

    #[test]
    fn parse_args_clamps_fps() {
        let Action::Run(opts) = parse_args(["--fps", "9999"]).unwrap() else {
            panic!();
        };
        assert_eq!(opts.fps, FPS_MAX);
        let Action::Run(opts) = parse_args(["--fps", "0"]).unwrap() else {
            panic!();
        };
        assert_eq!(opts.fps, FPS_MIN);
    }

    #[test]
    fn parse_args_rejects_unknown_and_missing() {
        assert!(parse_args(["--nope"]).is_err());
        assert!(parse_args(["--fade"]).is_err());
        assert!(parse_args(["--fade", "bad"]).is_err());
    }

    #[test]
    fn parse_duration_leading_dot() {
        // A bare-leading-dot decimal with a `s` suffix: `.5s` == 500ms.
        assert_eq!(parse_duration(".5s").unwrap(), Duration::from_millis(500));
    }

    #[test]
    fn parse_duration_surrounding_whitespace() {
        // Outer whitespace is trimmed; a bare number is milliseconds.
        assert_eq!(parse_duration(" 50 ").unwrap(), Duration::from_millis(50));
    }

    #[test]
    fn parse_duration_rejects_multiple_dots() {
        assert!(parse_duration("1.2.3").is_err());
    }

    #[test]
    fn parse_duration_rejects_non_finite() {
        // `inf`/`nan` parse as f64 but must be rejected as non-finite.
        assert!(parse_duration("inf").is_err());
        assert!(parse_duration("nan").is_err());
        assert!(parse_duration("NaN").is_err());
    }

    #[test]
    fn parse_duration_seconds_decimal_zero() {
        // Zero in either unit collapses to the zero duration.
        assert_eq!(parse_duration("0s").unwrap(), Duration::ZERO);
        assert_eq!(parse_duration("0ms").unwrap(), Duration::ZERO);
    }

    #[test]
    fn parse_color_case_insensitive() {
        // Hex letters are case-insensitive in both 6- and 3-digit forms.
        assert_eq!(
            parse_color("#FF00AA").unwrap(),
            parse_color("#ff00aa").unwrap()
        );
        assert_eq!(parse_color("#Ff0").unwrap(), Rgb(255, 255, 0));
    }

    #[test]
    fn parse_color_three_digit_extremes() {
        assert_eq!(parse_color("#000").unwrap(), Rgb(0, 0, 0));
        assert_eq!(parse_color("#fff").unwrap(), Rgb(255, 255, 255));
    }

    #[test]
    fn parse_color_rejects_4_and_5_digits() {
        // Only 3- or 6-digit hex is accepted.
        assert!(parse_color("#1234").is_err());
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn parse_color_rejects_bare_hash() {
        // A lone `#` strips to an empty (0-length) hex body.
        assert!(parse_color("#").is_err());
    }

    #[test]
    fn parse_args_rejects_non_integer_fps() {
        // `--fps` parses as a u32; decimals and letters are rejected.
        assert!(parse_args(["--fps", "1.5"]).is_err());
        assert!(parse_args(["--fps", "abc"]).is_err());
    }

    #[test]
    fn parse_args_last_flag_wins() {
        // Repeated flags overwrite earlier values (last-one-wins).
        let Action::Run(opts) = parse_args(["--fade", "100ms", "--fade", "200ms"]).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.fade, Duration::from_millis(200));
    }

    #[test]
    fn parse_args_accepts_equals_form() {
        // `--flag=value` is equivalent to `--flag value`.
        let Action::Run(opts) = parse_args([
            "--fade=200ms",
            "--stagger=30ms",
            "--from=#000",
            "--to=#fff",
            "--fps=30",
        ])
        .unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.fade, Duration::from_millis(200));
        assert_eq!(opts.stagger, Duration::from_millis(30));
        assert_eq!(opts.from, Rgb(0, 0, 0));
        assert_eq!(opts.to, Rgb(255, 255, 255));
        assert_eq!(opts.fps, 30);
    }

    #[test]
    fn parse_args_equals_form_empty_value_errors() {
        // `--fade=` supplies an empty value, which is an invalid duration.
        assert!(parse_args(["--fade="]).is_err());
    }

    #[test]
    fn parse_args_equals_form_value_with_equals() {
        // Only the first `=` splits flag from value; later `=` stay in the
        // value (colors/durations never contain `=`, but the rule is total).
        assert!(parse_args(["--fps=1=2"]).is_err());
    }

    #[test]
    fn parse_args_value_less_flag_rejects_inline_value() {
        // `--help`/`--version` take no value; a stray `=foo` must not be
        // silently discarded.
        assert!(parse_args(["--help=foo"]).is_err());
        assert!(parse_args(["--version=1"]).is_err());
        // The bare forms still work.
        assert_eq!(parse_args(["--help"]).unwrap(), Action::Help);
        assert_eq!(parse_args(["--version"]).unwrap(), Action::Version);
    }

    #[test]
    fn parse_args_read_flag() {
        // `--read` is a value-less bool; default is false.
        let Action::Run(opts) = parse_args(["--read"]).unwrap() else {
            panic!("expected Run");
        };
        assert!(opts.read);
        assert_eq!(opts.by, Unit::Sentence, "default unit is sentence");

        let Action::Run(opts) = parse_args::<[&str; 0], &str>([]).unwrap() else {
            panic!("expected Run");
        };
        assert!(!opts.read, "read defaults to false");
    }

    #[test]
    fn parse_args_by_units() {
        for (arg, want) in [
            ("sentence", Unit::Sentence),
            ("paragraph", Unit::Paragraph),
            ("line", Unit::Line),
        ] {
            let Action::Run(opts) = parse_args(["--by", arg]).unwrap() else {
                panic!("expected Run");
            };
            assert_eq!(opts.by, want);
        }
        // `=`-joined form works too.
        let Action::Run(opts) = parse_args(["--by=line"]).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.by, Unit::Line);
    }

    #[test]
    fn parse_args_by_rejects_bad_unit_and_read_rejects_inline() {
        // An unknown unit errors (caller exits 2).
        assert!(parse_args(["--by", "word"]).is_err());
        assert!(parse_args(["--by"]).is_err());
        // `--read` takes no value.
        assert!(parse_args(["--read=1"]).is_err());
    }

    #[test]
    fn parse_args_missing_value_for_each_flag() {
        // A trailing value-taking flag with no following argument errors.
        assert!(parse_args(["--from"]).is_err());
        assert!(parse_args(["--to"]).is_err());
        assert!(parse_args(["--stagger"]).is_err());
        assert!(parse_args(["--fps"]).is_err());
    }
}
