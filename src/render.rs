//! Tokenizer + frame builder for the `jiwa` binary.
//!
//! The CLI has to cope with input that already contains ANSI escape
//! sequences (e.g. `lolcat`-colored text). We split input into a flat
//! list of [`Token`]s — passthrough escape sequences and printable
//! grapheme clusters — and rebuild each animation frame as a pure
//! function of `(tokens, visible_count, colors)`. Keeping the builder
//! pure makes it directly unit-testable without a terminal.
//!
//! Binary-only: not referenced by `lib.rs`.

use jiwa::Rgb;
use unicode_segmentation::UnicodeSegmentation;

/// One unit of tokenized input.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// An ANSI escape sequence (`ESC [ ... <final byte>`). Zero display
    /// width; does not consume a reveal index. Emitted verbatim.
    Escape(String),
    /// A printable grapheme cluster (including `\n`). Consumes one reveal
    /// index.
    Grapheme(String),
}

/// Tokenized input plus the "did the input already carry color?" flag.
#[derive(Debug, Clone, PartialEq)]
pub struct Tokens {
    pub tokens: Vec<Token>,
    /// True if at least one escape sequence was present — i.e. the input
    /// is already colored (lolcat etc.) and jiwa must not impose its own
    /// foreground.
    pub has_input_color: bool,
}

/// Split `input` into escape-sequence and grapheme tokens.
///
/// An escape sequence is recognized as CSI form: `\x1b[` followed by zero
/// or more parameter/intermediate bytes and terminated by a final byte in
/// the range `@`..=`~` (0x40..=0x7e), which covers SGR (`m`) and friends.
/// A lone or malformed `\x1b` that does not match is treated as ordinary
/// text so we never silently drop bytes.
pub fn tokenize(input: &str) -> Tokens {
    let mut tokens = Vec::new();
    let mut has_input_color = false;
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < input.len() {
        if bytes[i] == 0x1b && i + 1 < input.len() && bytes[i + 1] == b'[' {
            // Scan for the CSI final byte.
            let mut j = i + 2;
            while j < input.len() {
                let b = bytes[j];
                if (0x40..=0x7e).contains(&b) {
                    // Final byte found (inclusive).
                    j += 1;
                    break;
                }
                j += 1;
            }
            // A well-formed CSI ends on a final byte. If we ran off the
            // end without one, fall through to grapheme handling so the
            // raw bytes are not lost.
            let last = bytes[j - 1];
            if j > i + 2 && (0x40..=0x7e).contains(&last) {
                tokens.push(Token::Escape(input[i..j].to_string()));
                has_input_color = true;
                i = j;
                continue;
            }
        }

        // Not an escape: consume one grapheme cluster starting at `i`.
        let rest = &input[i..];
        if let Some(g) = rest.graphemes(true).next() {
            tokens.push(Token::Grapheme(g.to_string()));
            i += g.len();
        } else {
            break;
        }
    }

    Tokens {
        tokens,
        has_input_color,
    }
}

/// The plain (escape-free) text used to drive reveal timing. This is what
/// gets handed to `RevealHandle::start_at`.
pub fn plain_text(tokens: &Tokens) -> String {
    let mut out = String::new();
    for t in &tokens.tokens {
        if let Token::Grapheme(g) = t {
            out.push_str(g);
        }
    }
    out
}

/// Build one frame string.
///
/// - `visible_count` graphemes (counting from the front) are shown.
/// - `colors[i]` is the foreground for the i-th *printable* grapheme,
///   used only when `has_input_color == false`.
/// - Escape tokens are emitted as long as a visible grapheme still
///   follows them (so color state is set before the grapheme it governs),
///   but trailing escapes after the last visible grapheme are dropped.
/// - Always terminated with a reset (`\x1b[0m`).
pub fn render_frame(
    tokens: &[Token],
    visible_count: usize,
    colors: &[Rgb],
    has_input_color: bool,
) -> String {
    // Index (into `tokens`) just past the last printable grapheme we will
    // show. Escapes beyond this point are suppressed.
    let mut last_visible_token_idx = 0usize;
    {
        let mut seen = 0usize;
        for (idx, t) in tokens.iter().enumerate() {
            if let Token::Grapheme(_) = t {
                if seen < visible_count {
                    last_visible_token_idx = idx + 1;
                    seen += 1;
                } else {
                    break;
                }
            }
        }
    }

    let mut out = String::new();
    let mut grapheme_idx = 0usize;

    for (idx, t) in tokens.iter().enumerate() {
        if idx >= last_visible_token_idx {
            break;
        }
        match t {
            Token::Escape(seq) => {
                // Only emitted because a visible grapheme follows (the
                // loop bound guarantees idx < last_visible_token_idx).
                out.push_str(seq);
            }
            Token::Grapheme(g) => {
                if grapheme_idx < visible_count {
                    if !has_input_color {
                        if let Some(c) = colors.get(grapheme_idx) {
                            out.push_str(&format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2));
                        }
                    }
                    out.push_str(g);
                    grapheme_idx += 1;
                }
            }
        }
    }

    out.push_str("\x1b[0m");
    out
}

/// Count the newlines among the first `visible_count` printable graphemes.
/// Used by the I/O loop to know how many rows the previous frame occupied.
pub fn visible_newline_rows(tokens: &[Token], visible_count: usize) -> usize {
    let mut seen = 0usize;
    let mut rows = 0usize;
    for t in tokens {
        if let Token::Grapheme(g) = t {
            if seen >= visible_count {
                break;
            }
            if g == "\n" {
                rows += 1;
            }
            seen += 1;
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_plain_text_has_no_color() {
        let t = tokenize("ab\nc");
        assert!(!t.has_input_color);
        assert_eq!(
            t.tokens,
            vec![
                Token::Grapheme("a".into()),
                Token::Grapheme("b".into()),
                Token::Grapheme("\n".into()),
                Token::Grapheme("c".into()),
            ]
        );
        assert_eq!(plain_text(&t), "ab\nc");
    }

    #[test]
    fn tokenize_grapheme_clusters() {
        let t = tokenize("e\u{0301}世");
        assert_eq!(
            t.tokens,
            vec![
                Token::Grapheme("e\u{0301}".into()),
                Token::Grapheme("世".into()),
            ]
        );
    }

    #[test]
    fn tokenize_splits_escapes_and_sets_flag() {
        let t = tokenize("\x1b[31mA\x1b[0m");
        assert!(t.has_input_color);
        assert_eq!(
            t.tokens,
            vec![
                Token::Escape("\x1b[31m".into()),
                Token::Grapheme("A".into()),
                Token::Escape("\x1b[0m".into()),
            ]
        );
        // Escapes do not count toward plain text used for timing.
        assert_eq!(plain_text(&t), "A");
    }

    #[test]
    fn render_frame_colors_each_grapheme_when_no_input_color() {
        let t = tokenize("ab");
        let colors = [Rgb(10, 20, 30), Rgb(40, 50, 60)];
        let frame = render_frame(&t.tokens, 1, &colors, false);
        assert_eq!(frame, "\x1b[38;2;10;20;30ma\x1b[0m");

        let frame = render_frame(&t.tokens, 2, &colors, false);
        assert_eq!(frame, "\x1b[38;2;10;20;30ma\x1b[38;2;40;50;60mb\x1b[0m");
    }

    #[test]
    fn render_frame_passthrough_preserves_input_escapes() {
        let t = tokenize("\x1b[31mA\x1b[0m");
        // has_input_color = true -> no jiwa foreground; trailing escape
        // after the last visible grapheme is dropped, only reset remains.
        let frame = render_frame(&t.tokens, 1, &[], true);
        assert_eq!(frame, "\x1b[31mA\x1b[0m");
    }

    #[test]
    fn render_frame_hides_beyond_visible_count() {
        let t = tokenize("abc");
        let colors = [Rgb(0, 0, 0); 3];
        let frame = render_frame(&t.tokens, 0, &colors, false);
        assert_eq!(frame, "\x1b[0m");
    }

    #[test]
    fn visible_newline_rows_counts_only_visible() {
        let t = tokenize("a\nb\nc");
        // graphemes: a \n b \n c  -> first 3 contain one newline
        assert_eq!(visible_newline_rows(&t.tokens, 3), 1);
        assert_eq!(visible_newline_rows(&t.tokens, 5), 2);
        assert_eq!(visible_newline_rows(&t.tokens, 1), 0);
    }

    #[test]
    fn tokenize_incomplete_escape_not_dropped() {
        // `\x1b[31` ends without a CSI final byte. It must be kept as text
        // (not silently dropped) and must not panic.
        let t = tokenize("\x1b[31");
        // No well-formed escape -> input was not treated as colored.
        assert!(!t.has_input_color);
        // Every input byte survives into the plain text.
        assert_eq!(plain_text(&t), "\x1b[31");
    }

    #[test]
    fn tokenize_lone_esc_byte() {
        // A bare ESC, and an ESC sandwiched between letters, are kept verbatim.
        assert_eq!(plain_text(&tokenize("\x1b")), "\x1b");
        assert_eq!(plain_text(&tokenize("a\x1bb")), "a\x1bb");
    }

    #[test]
    fn tokenize_consecutive_escapes() {
        // Two back-to-back CSI sequences then one grapheme.
        let t = tokenize("\x1b[1m\x1b[31mA");
        assert_eq!(
            t.tokens,
            vec![
                Token::Escape("\x1b[1m".into()),
                Token::Escape("\x1b[31m".into()),
                Token::Grapheme("A".into()),
            ]
        );
    }

    #[test]
    fn tokenize_zwj_emoji_single_grapheme() {
        // A ZWJ family emoji is one grapheme cluster, not its components.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let t = tokenize(family);
        assert_eq!(t.tokens, vec![Token::Grapheme(family.into())]);
    }

    #[test]
    fn tokenize_empty_input() {
        let t = tokenize("");
        assert!(t.tokens.is_empty());
        assert!(!t.has_input_color);
        assert_eq!(plain_text(&t), "");
    }

    #[test]
    fn render_frame_drops_trailing_escape_after_last_visible() {
        // `A <esc> B` with only A visible: the escape after the last visible
        // grapheme is suppressed (no following visible grapheme).
        let t = tokenize("A\x1b[0mB");
        let frame = render_frame(&t.tokens, 1, &[Rgb(1, 2, 3)], false);
        assert_eq!(frame, "\x1b[38;2;1;2;3mA\x1b[0m");
    }

    #[test]
    fn render_frame_emits_escape_before_following_visible() {
        // A leading escape is emitted because a visible grapheme follows it.
        let t = tokenize("\x1b[31mAB");
        let frame = render_frame(&t.tokens, 2, &[], true);
        assert_eq!(frame, "\x1b[31mAB\x1b[0m");
    }

    #[test]
    fn render_frame_colors_shorter_than_visible_no_panic() {
        // Fewer colors than visible graphemes: missing entries simply emit
        // no foreground (no out-of-bounds panic).
        let t = tokenize("ab");
        let frame = render_frame(&t.tokens, 2, &[], false);
        assert_eq!(frame, "ab\x1b[0m");
    }

    #[test]
    fn render_frame_has_input_color_suppresses_jiwa_fg() {
        // With input color present, jiwa never injects its own `38;2` fg,
        // even given a colors slice and multiple visible graphemes.
        let t = tokenize("\x1b[31mABC");
        let colors = [Rgb(10, 20, 30); 3];
        let frame = render_frame(&t.tokens, 3, &colors, true);
        assert!(!frame.contains("38;2"));
        assert_eq!(frame, "\x1b[31mABC\x1b[0m");
    }

    #[test]
    fn visible_newline_rows_zero_and_all() {
        // Zero visible -> zero rows; consecutive and trailing newlines count.
        let t = tokenize("\n\na");
        assert_eq!(visible_newline_rows(&t.tokens, 0), 0);
        assert_eq!(visible_newline_rows(&t.tokens, 2), 2);
        assert_eq!(visible_newline_rows(&t.tokens, 3), 2);
    }

    #[test]
    fn plain_text_excludes_all_escapes_multi() {
        // Escapes interleaved with multibyte graphemes vanish from plain text.
        let t = tokenize("\x1b[31m世\x1b[0m界");
        assert_eq!(plain_text(&t), "世界");
    }
}
