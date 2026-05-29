//! Interactive reader (sound-novel) mode for the `jiwa` binary.
//!
//! Phase 1: split the piped text into segments (sentence / paragraph /
//! line), reveal each one, and wait for Enter on `/dev/tty` before the
//! next. Dependency-free — no raw mode, no termios; just line-buffered
//! reads from the controlling terminal, the same trick `less` / `fzf` /
//! `git add -p` use when stdin is occupied by piped data.
//!
//! The segmentation here is a pure function so it can be unit-tested
//! without a terminal. The I/O loop (`run_reader`) lives in `main.rs`
//! because it reuses the binary's existing reveal/cursor machinery.
//!
//! Binary-only: not referenced by `lib.rs`.

use unicode_segmentation::UnicodeSegmentation;

/// How reader mode carves the input into segments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    /// One sentence at a time (Japanese 。！？ + trailing close brackets,
    /// English `. ! ?` followed by space/newline/end).
    Sentence,
    /// One paragraph (text between blank lines) at a time.
    Paragraph,
    /// One line (`\n`-delimited) at a time.
    Line,
}

/// Closing punctuation that, when it immediately follows a sentence-ending
/// mark, is pulled into the same sentence (`「…。」` stays one segment).
const CLOSERS: &[char] = &['」', '』', '）', ')', '"', '\'', '”', '’'];

/// Split `text` into reveal segments according to `unit`.
///
/// Segment boundaries keep their terminating punctuation. Empty segments
/// (whitespace-only after trimming) are dropped, but interior whitespace
/// and newlines are preserved so each segment reads naturally.
pub fn segment(text: &str, unit: Unit) -> Vec<String> {
    // Normalize newlines first so the splitters never see `\r`: CRLF (`\r\n`)
    // and classic-Mac lone `\r` both collapse to `\n`. Without this, paragraph
    // mode would not split on CRLF blank lines and sentence/line segments
    // could carry a stray `\r`.
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match unit {
        Unit::Sentence => segment_sentences(&normalized),
        Unit::Paragraph => segment_paragraphs(&normalized),
        Unit::Line => segment_lines(&normalized),
    }
}

/// Sentence splitter (see [`Unit::Sentence`]).
///
/// We walk grapheme clusters, accumulating into the current sentence. A
/// Japanese terminator (`。！？`) ends the sentence; any run of closing
/// brackets/quotes immediately after it is absorbed into the same
/// sentence. An ASCII `.`/`!`/`?` ends the sentence only when the next
/// character is whitespace, a newline, or the end of input — and, for
/// `.`, not when sandwiched between digits (so `3.14` is not split).
fn segment_sentences(text: &str) -> Vec<String> {
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    let mut out = Vec::new();
    let mut cur = String::new();

    let mut i = 0;
    while i < graphemes.len() {
        let g = graphemes[i];
        cur.push_str(g);

        let is_ja_end = g == "。" || g == "！" || g == "？";
        let is_ascii_end = g == "." || g == "!" || g == "?";

        let mut boundary = false;
        if is_ja_end {
            // Absorb any trailing closers (e.g. 」』）) into this sentence.
            while i + 1 < graphemes.len() && is_closer(graphemes[i + 1]) {
                i += 1;
                cur.push_str(graphemes[i]);
            }
            boundary = true;
        } else if is_ascii_end {
            let next = graphemes.get(i + 1).copied();
            // For `.`, avoid splitting decimals like `3.14`: if it sits
            // between two digits, it is not a sentence end.
            let decimal_dot = g == "."
                && i > 0
                && is_ascii_digit(graphemes[i - 1])
                && next.is_some_and(is_ascii_digit);
            // Look past any run of closing brackets/quotes (e.g. the `"`
            // in `"hi."`) to the first following character: that is what
            // decides whether the sentence actually ends here.
            let mut after = i + 1;
            while after < graphemes.len() && is_closer(graphemes[after]) {
                after += 1;
            }
            // English sentence end: the next non-closer is whitespace /
            // newline / end of input.
            let followed_by_break = match graphemes.get(after) {
                None => true,
                Some(n) => n.chars().all(char::is_whitespace),
            };
            if !decimal_dot && followed_by_break {
                // Absorb the closers we skipped over into this sentence.
                while i + 1 < after {
                    i += 1;
                    cur.push_str(graphemes[i]);
                }
                boundary = true;
            }
        }

        if boundary {
            push_trimmed(&mut out, &cur);
            cur.clear();
        }
        i += 1;
    }

    // Trailing fragment with no terminator is still a segment.
    push_trimmed(&mut out, &cur);
    out
}

/// Paragraph splitter: break on blank lines (two-or-more consecutive
/// newlines). Single newlines inside a paragraph are preserved.
fn segment_paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // Track consecutive newlines so 2+ in a row ends the paragraph.
    let mut newline_run = 0usize;

    for ch in text.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run == 2 {
                // Blank line reached: close the current paragraph. The
                // single `\n` pushed on the first newline of this run is a
                // trailing break, not an interior one, so drop it.
                if cur.ends_with('\n') {
                    cur.pop();
                }
                if !cur.is_empty() {
                    push_trimmed(&mut out, &cur);
                    cur.clear();
                }
                continue;
            }
            if newline_run > 2 {
                // Still inside the blank run; nothing to accumulate.
                continue;
            }
            cur.push(ch);
        } else {
            // A non-newline after a lone `\n` keeps that single newline as
            // an interior line break.
            newline_run = 0;
            cur.push(ch);
        }
    }
    push_trimmed(&mut out, &cur);
    out
}

/// Line splitter: one segment per `\n`-delimited line, trailing blank
/// lines dropped (via the trim-empty rule shared by all splitters).
fn segment_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        push_trimmed(&mut out, line);
    }
    out
}

/// Push `seg` onto `out` unless it is empty after trimming. The trim is
/// only used for the empty check; the original `seg` (with its natural
/// leading/trailing whitespace) is what gets stored.
fn push_trimmed(out: &mut Vec<String>, seg: &str) {
    if !seg.trim().is_empty() {
        out.push(seg.to_string());
    }
}

fn is_closer(g: &str) -> bool {
    let mut chars = g.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => CLOSERS.contains(&c),
        _ => false,
    }
}

fn is_ascii_digit(g: &str) -> bool {
    g.len() == 1 && g.as_bytes()[0].is_ascii_digit()
}

/// Build the dim one-line "press Enter to continue" prompt shown between
/// segments. `index` is 1-based; `total` is the segment count. Wrapped in
/// SGR dim (`\x1b[2m` … `\x1b[0m`); the caller erases it after Enter.
pub fn reader_prompt(index: usize, total: usize) -> String {
    format!("\x1b[2m[ {index}/{total} ] Enter \u{25b8}\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentence_japanese_basic() {
        assert_eq!(
            segment("一文目。二文目！三文目？", Unit::Sentence),
            vec!["一文目。", "二文目！", "三文目？"]
        );
    }

    #[test]
    fn sentence_japanese_absorbs_closing_bracket() {
        // The 」 after 。 belongs to the same sentence.
        assert_eq!(
            segment("「こんにちは。」次へ。", Unit::Sentence),
            vec!["「こんにちは。」", "次へ。"]
        );
    }

    #[test]
    fn sentence_english_splits_on_space_after_period() {
        assert_eq!(
            segment("Hello there. How are you?", Unit::Sentence),
            vec!["Hello there.", " How are you?"]
        );
    }

    #[test]
    fn sentence_does_not_split_decimal() {
        // `3.14` keeps its dot; the trailing `.` (end of input) still ends.
        assert_eq!(
            segment("Pi is 3.14 today.", Unit::Sentence),
            vec!["Pi is 3.14 today."]
        );
    }

    #[test]
    fn sentence_trailing_fragment_without_terminator() {
        assert_eq!(
            segment("First. Loose end", Unit::Sentence),
            vec!["First.", " Loose end"]
        );
    }

    #[test]
    fn sentence_empty_and_whitespace_only() {
        assert!(segment("", Unit::Sentence).is_empty());
        assert!(segment("   \n  ", Unit::Sentence).is_empty());
    }

    #[test]
    fn sentence_absorbs_ascii_quote() {
        // Closing `"` after `.` is pulled into the sentence.
        assert_eq!(
            segment("He said \"hi.\" Then left.", Unit::Sentence),
            vec!["He said \"hi.\"", " Then left."]
        );
    }

    #[test]
    fn paragraph_splits_on_blank_line() {
        assert_eq!(
            segment("Para one.\nStill one.\n\nPara two.", Unit::Paragraph),
            vec!["Para one.\nStill one.", "Para two."]
        );
    }

    #[test]
    fn paragraph_collapses_multiple_blank_lines() {
        assert_eq!(segment("A\n\n\n\nB", Unit::Paragraph), vec!["A", "B"]);
    }

    #[test]
    fn line_splits_on_newline_drops_trailing_blanks() {
        assert_eq!(segment("one\ntwo\n\n", Unit::Line), vec!["one", "two"]);
    }

    #[test]
    fn line_drops_empty_interior_lines() {
        // Blank interior lines are trimmed-empty and dropped.
        assert_eq!(segment("a\n\nb", Unit::Line), vec!["a", "b"]);
    }

    #[test]
    fn reader_prompt_is_dim_and_has_counts() {
        let p = reader_prompt(2, 5);
        assert!(p.starts_with("\x1b[2m"), "starts dim");
        assert!(p.ends_with("\x1b[0m"), "ends with reset");
        assert!(p.contains("2/5"), "shows index/total");
    }
}
