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

    // --- Sentence: additional edge cases ---

    #[test]
    fn sentence_mr_period_splits_on_space() {
        // Current spec: `.` + following space ends the sentence even after
        // an abbreviation like "Mr." (no abbreviation dictionary).
        assert_eq!(
            segment("Mr. Smith went home.", Unit::Sentence),
            vec!["Mr.", " Smith went home."]
        );
    }

    #[test]
    fn sentence_ellipsis_midword() {
        assert_eq!(
            segment("Wait... really.", Unit::Sentence),
            vec!["Wait...", " really."]
        );
    }

    #[test]
    fn sentence_ellipsis_trailing() {
        assert_eq!(segment("Wait...", Unit::Sentence), vec!["Wait..."]);
    }

    #[test]
    fn sentence_consecutive_japanese_terminators() {
        // Each Japanese terminator ends a sentence, so a doubled 。 yields a
        // standalone "。" segment.
        assert_eq!(
            segment("本当。。終わり。", Unit::Sentence),
            vec!["本当。", "。", "終わり。"]
        );
    }

    #[test]
    fn sentence_mixed_bang_question() {
        assert_eq!(
            segment("Really?! Yes.", Unit::Sentence),
            vec!["Really?!", " Yes."]
        );
    }

    #[test]
    fn sentence_leading_dot_decimal_like() {
        // ".5" has no preceding digit, so the leading dot is not a decimal
        // separator; the only sentence end is the trailing `.` at EOF.
        assert_eq!(segment(".5 cents.", Unit::Sentence), vec![".5 cents."]);
    }

    #[test]
    fn sentence_digit_then_terminal_dot() {
        // Preceding digit but EOF after the dot (no following digit) -> the
        // dot terminates the sentence.
        assert_eq!(segment("100.", Unit::Sentence), vec!["100."]);
    }

    #[test]
    fn sentence_emoji_before_terminator() {
        // A multi-codepoint grapheme (emoji) right before the `.` is kept in
        // the sentence; the `.`+space still ends it.
        assert_eq!(
            segment("Run🎉. Next.", Unit::Sentence),
            vec!["Run🎉.", " Next."]
        );
    }

    #[test]
    fn sentence_crlf_normalized_no_cr_residue() {
        // must-2 regression guard: after CRLF normalization no `\r` survives
        // in any segment; the boundary is `.` followed by the normalized `\n`.
        assert_eq!(
            segment("Line one.\r\nLine two.", Unit::Sentence),
            vec!["Line one.", "\nLine two."]
        );
    }

    #[test]
    fn sentence_domain_dot_not_split() {
        // A `.` followed by a non-whitespace char does not end the sentence,
        // so domains/abbreviations stay intact until a space or EOF.
        assert_eq!(segment("a.b", Unit::Sentence), vec!["a.b"]);
        assert_eq!(
            segment("U.S.A. is here.", Unit::Sentence),
            vec!["U.S.A.", " is here."]
        );
    }

    #[test]
    fn sentence_closer_at_eof() {
        // A terminator followed only by closers + EOF stays one segment.
        assert_eq!(
            segment("He said \"no.\"", Unit::Sentence),
            vec!["He said \"no.\""]
        );
        assert_eq!(segment("end.)", Unit::Sentence), vec!["end.)"]);
    }

    #[test]
    fn sentence_closer_then_space() {
        // The closer is absorbed, then the following space confirms the end.
        assert_eq!(segment("Hi.) Bye.", Unit::Sentence), vec!["Hi.)", " Bye."]);
    }

    #[test]
    fn sentence_japanese_terminator_then_newline() {
        // 。 ends the sentence; the interior newline starts the next segment.
        assert_eq!(segment("a。\nb", Unit::Sentence), vec!["a。", "\nb"]);
    }

    #[test]
    fn sentence_combining_grapheme_preserved() {
        // "é" written as e + combining acute is one grapheme and stays whole.
        assert_eq!(
            segment("e\u{0301}nd.", Unit::Sentence),
            vec!["e\u{0301}nd."]
        );
    }

    #[test]
    fn sentence_single_terminator_only() {
        assert_eq!(segment("。", Unit::Sentence), vec!["。"]);
    }

    #[test]
    fn sentence_unclosed_opener() {
        // A leading opener without a matching closer does not interfere; the
        // sentence still ends at 。 (EOF).
        assert_eq!(segment("「終わり。", Unit::Sentence), vec!["「終わり。"]);
    }

    // --- Paragraph / Line: additional edge cases ---

    #[test]
    fn paragraph_preserves_interior_single_newline() {
        assert_eq!(
            segment("A\nB\n\nC\nD", Unit::Paragraph),
            vec!["A\nB", "C\nD"]
        );
    }

    #[test]
    fn paragraph_leading_blank_lines_dropped() {
        assert_eq!(segment("\n\nA", Unit::Paragraph), vec!["A"]);
    }

    #[test]
    fn paragraph_whitespace_only_line_between() {
        // Measured: a line containing only spaces ("  ") is non-newline
        // content that resets the newline run, so the two halves stay joined
        // into a single paragraph (interior whitespace preserved).
        assert_eq!(segment("A\n  \nB", Unit::Paragraph), vec!["A\n  \nB"]);
    }

    #[test]
    fn paragraph_crlf_blank_line_splits() {
        // must-1 regression guard: a CRLF blank line must split paragraphs.
        assert_eq!(segment("a\r\n\r\nb", Unit::Paragraph), vec!["a", "b"]);
    }

    #[test]
    fn line_last_line_without_newline() {
        assert_eq!(segment("a\nb", Unit::Line), vec!["a", "b"]);
    }

    #[test]
    fn line_crlf_normalized() {
        // After CRLF normalization the trailing blank line is dropped and no
        // `\r` survives in the segments.
        assert_eq!(segment("a\r\nb\r\n\r\n", Unit::Line), vec!["a", "b"]);
    }

    #[test]
    fn line_all_blank_inputs() {
        assert!(segment("", Unit::Line).is_empty());
        assert!(segment("\n", Unit::Line).is_empty());
    }

    #[test]
    fn reader_prompt_index_equals_total_and_arrow() {
        let p = reader_prompt(1, 1);
        assert!(p.contains("1/1"), "shows index/total when equal");
        assert!(p.contains('\u{25b8}'), "contains the advance arrow");
    }

    #[test]
    fn reader_prompt_is_dim_and_has_counts() {
        let p = reader_prompt(2, 5);
        assert!(p.starts_with("\x1b[2m"), "starts dim");
        assert!(p.ends_with("\x1b[0m"), "ends with reset");
        assert!(p.contains("2/5"), "shows index/total");
    }
}
