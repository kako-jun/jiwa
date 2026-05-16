//! Typewriter + per-grapheme fade-in.
//!
//! Given a target `text` and a [`RevealOpts`], a [`RevealHandle`] reports
//! at any wall-clock instant which graphemes are currently visible and
//! what color each one should render at. It does no I/O, owns no thread,
//! and never sleeps: callers tick it at their existing redraw cadence.
//!
//! # Timing model
//!
//! - The first grapheme is visible the moment the reveal starts (`t=0`).
//! - Each subsequent grapheme appears `char_interval` later than the one
//!   before it (`t_i = i * char_interval`).
//! - When a grapheme appears it carries `fade_from` color; over the next
//!   `fade_duration` it linearly interpolates to `fade_to`. After that
//!   it stays at `fade_to`.
//!
//! Setting `char_interval = 0` makes every grapheme appear at `t=0` —
//! a whole-block fade-in instead of a typewriter. Setting
//! `fade_duration = 0` makes each grapheme appear at its final color
//! instantly — a pure typewriter without the fade.
//!
//! # Why grapheme clusters
//!
//! Iterating Japanese text by `char` happens to work for kanji because
//! one kanji = one Unicode scalar, but combining marks (e.g. `é` written
//! as `e` + U+0301) and ZWJ emoji sequences (`👨‍👩‍👧‍👦`) would split
//! visually-single characters into multiple frames. `unicode-segmentation`
//! gives us proper extended grapheme clusters per UAX #29.

use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

use crate::color::{lerp_rgb, Rgb};

/// One grapheme as it appears at a particular instant during the reveal.
#[derive(Debug, Clone, PartialEq)]
pub struct RevealedGrapheme {
    /// The actual text segment the renderer should draw.
    pub text: String,
    /// Foreground color at the snapshot time.
    pub color: Rgb,
    /// Linear progress from 0.0 (just appeared, color = `fade_from`) to
    /// 1.0 (fade complete, color = `fade_to`).
    pub progress: f32,
}

/// Tunables for one reveal animation.
#[derive(Debug, Clone, Copy)]
pub struct RevealOpts {
    /// Time between successive grapheme appearances ("typewriter speed").
    /// Zero means "all graphemes appear at once" — useful for a
    /// whole-block fade-in.
    pub char_interval: Duration,
    /// How long each grapheme spends fading from `fade_from` to `fade_to`.
    /// Zero means "instantly at `fade_to`" — useful for a pure
    /// typewriter without the fade.
    pub fade_duration: Duration,
    pub fade_from: Rgb,
    pub fade_to: Rgb,
}

impl RevealOpts {
    /// Comfortable preset for problem text on a dark terminal:
    /// 45 ms typewriter step, 320 ms fade from a deep gray to a soft
    /// green. The in-between color reads as an afterimage instead of
    /// a blink.
    pub const fn soft_green() -> Self {
        Self {
            char_interval: Duration::from_millis(45),
            fade_duration: Duration::from_millis(320),
            fade_from: Rgb(60, 60, 60),
            fade_to: Rgb(160, 220, 160),
        }
    }
}

impl Default for RevealOpts {
    /// Same as [`RevealOpts::soft_green`].
    fn default() -> Self {
        Self::soft_green()
    }
}

/// One in-flight reveal. Cheap to construct (`O(graphemes)`); tick by
/// calling [`RevealHandle::snapshot`] each frame.
#[derive(Debug)]
pub struct RevealHandle {
    graphemes: Vec<String>,
    started_at: Instant,
    opts: RevealOpts,
}

impl RevealHandle {
    /// Start a reveal anchored to `now`. Use [`RevealHandle::start`] in
    /// production code; tests should pass an explicit `now` so they can
    /// step time without sleeping.
    pub fn start_at(text: &str, opts: RevealOpts, now: Instant) -> Self {
        let graphemes = text.graphemes(true).map(|g| g.to_string()).collect();
        Self {
            graphemes,
            started_at: now,
            opts,
        }
    }

    /// Convenience for production callers: anchors at `Instant::now()`.
    pub fn start(text: &str, opts: RevealOpts) -> Self {
        Self::start_at(text, opts, Instant::now())
    }

    /// Total grapheme count of the source text.
    pub fn total_graphemes(&self) -> usize {
        self.graphemes.len()
    }

    /// How many graphemes are visible at `now` (regardless of whether
    /// they have finished fading).
    pub fn visible_count(&self, now: Instant) -> usize {
        if self.graphemes.is_empty() {
            return 0;
        }
        if self.opts.char_interval.is_zero() {
            return self.graphemes.len();
        }
        let interval_nanos = self.opts.char_interval.as_nanos();
        let elapsed_nanos = now.saturating_duration_since(self.started_at).as_nanos();
        // First grapheme is visible at t=0 → +1 against the floor div.
        let by_time = (elapsed_nanos / interval_nanos) as usize + 1;
        by_time.min(self.graphemes.len())
    }

    /// True once every grapheme has both appeared *and* finished fading.
    /// Useful as a "ready for input" trigger or to stop ticking the
    /// snapshot loop.
    pub fn is_done(&self, now: Instant) -> bool {
        if self.graphemes.is_empty() {
            return true;
        }
        let total_appearance =
            self.opts.char_interval * (self.graphemes.len().saturating_sub(1) as u32);
        let total_runtime = total_appearance + self.opts.fade_duration;
        now.saturating_duration_since(self.started_at) >= total_runtime
    }

    /// Snapshot the reveal at `now`. Hidden graphemes are simply absent
    /// from the returned vec — the caller appends a cursor / placeholder
    /// itself if it wants one.
    pub fn snapshot(&self, now: Instant) -> Vec<RevealedGrapheme> {
        let visible = self.visible_count(now);
        let elapsed = now.saturating_duration_since(self.started_at);
        let mut out = Vec::with_capacity(visible);
        for i in 0..visible {
            let appearance = self.opts.char_interval * (i as u32);
            let age = elapsed.saturating_sub(appearance);
            let progress = fade_progress(age, self.opts.fade_duration);
            let color = lerp_rgb(self.opts.fade_from, self.opts.fade_to, progress);
            out.push(RevealedGrapheme {
                text: self.graphemes[i].clone(),
                color,
                progress,
            });
        }
        out
    }
}

/// Linear progress in `[0.0, 1.0]` for a grapheme that has been visible
/// for `age` against a fade window of `fade`. `fade == 0` is treated as
/// "instantly fully faded" so callers can opt out of the fade by zeroing
/// it without a divide-by-zero.
fn fade_progress(age: Duration, fade: Duration) -> f32 {
    if fade.is_zero() {
        return 1.0;
    }
    let raw = age.as_secs_f64() / fade.as_secs_f64();
    raw.clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RevealOpts {
        RevealOpts {
            char_interval: Duration::from_millis(50),
            fade_duration: Duration::from_millis(100),
            fade_from: Rgb(0, 0, 0),
            fade_to: Rgb(200, 200, 200),
        }
    }

    fn epoch() -> Instant {
        Instant::now()
    }

    #[test]
    fn first_grapheme_is_visible_at_t_zero() {
        let now = epoch();
        let h = RevealHandle::start_at("abc", opts(), now);
        let snap = h.snapshot(now);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].text, "a");
        assert_eq!(snap[0].color, Rgb(0, 0, 0));
        assert_eq!(snap[0].progress, 0.0);
    }

    #[test]
    fn typewriter_advances_one_grapheme_per_interval() {
        let now = epoch();
        let h = RevealHandle::start_at("abcd", opts(), now);
        assert_eq!(h.visible_count(now), 1);
        assert_eq!(h.visible_count(now + Duration::from_millis(50)), 2);
        assert_eq!(h.visible_count(now + Duration::from_millis(100)), 3);
        assert_eq!(h.visible_count(now + Duration::from_millis(150)), 4);
        assert_eq!(h.visible_count(now + Duration::from_millis(500)), 4);
    }

    #[test]
    fn fade_interpolates_over_fade_duration() {
        let now = epoch();
        let h = RevealHandle::start_at("a", opts(), now);

        let snap = h.snapshot(now + Duration::from_millis(50));
        assert!((snap[0].progress - 0.5).abs() < 1e-3);
        assert_eq!(snap[0].color, Rgb(100, 100, 100));

        let snap = h.snapshot(now + Duration::from_millis(100));
        assert_eq!(snap[0].progress, 1.0);
        assert_eq!(snap[0].color, Rgb(200, 200, 200));

        let snap = h.snapshot(now + Duration::from_millis(400));
        assert_eq!(snap[0].color, Rgb(200, 200, 200));
    }

    #[test]
    fn later_graphemes_start_their_own_fade() {
        let now = epoch();
        let h = RevealHandle::start_at("ab", opts(), now);
        let snap = h.snapshot(now + Duration::from_millis(50));
        assert_eq!(snap.len(), 2);
        assert!((snap[0].progress - 0.5).abs() < 1e-3);
        assert_eq!(snap[1].progress, 0.0);
    }

    #[test]
    fn iterates_grapheme_clusters_not_chars() {
        let composed = "e\u{0301}f"; // "éf"
        let now = epoch();
        let h = RevealHandle::start_at(composed, opts(), now);
        assert_eq!(h.total_graphemes(), 2);

        let snap = h.snapshot(now);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].text, "e\u{0301}");
    }

    #[test]
    fn handles_japanese_text() {
        let now = epoch();
        let h = RevealHandle::start_at("東京特許許可局", opts(), now);
        assert_eq!(h.total_graphemes(), 7);

        let snap = h.snapshot(now + Duration::from_millis(150));
        assert_eq!(snap.len(), 4);
        assert_eq!(snap[0].text, "東");
        assert_eq!(snap[3].text, "許");
    }

    #[test]
    fn empty_text_has_no_graphemes() {
        let now = epoch();
        let h = RevealHandle::start_at("", opts(), now);
        assert_eq!(h.total_graphemes(), 0);
        assert_eq!(h.visible_count(now), 0);
        assert!(h.is_done(now));
        assert!(h.snapshot(now).is_empty());
    }

    #[test]
    fn is_done_reports_when_last_grapheme_finishes_fading() {
        let now = epoch();
        let h = RevealHandle::start_at("abc", opts(), now);
        assert!(!h.is_done(now));
        assert!(!h.is_done(now + Duration::from_millis(150)));
        assert!(!h.is_done(now + Duration::from_millis(199)));
        assert!(h.is_done(now + Duration::from_millis(200)));
        assert!(h.is_done(now + Duration::from_millis(500)));
    }

    #[test]
    fn zero_fade_produces_instant_full_color() {
        let mut o = opts();
        o.fade_duration = Duration::ZERO;
        let now = epoch();
        let h = RevealHandle::start_at("a", o, now);
        let snap = h.snapshot(now);
        assert_eq!(snap[0].progress, 1.0);
        assert_eq!(snap[0].color, o.fade_to);
    }

    #[test]
    fn zero_char_interval_reveals_all_at_once() {
        let mut o = opts();
        o.char_interval = Duration::ZERO;
        let now = epoch();
        let h = RevealHandle::start_at("abcd", o, now);
        // All graphemes are visible at t=0, all starting their fade together.
        assert_eq!(h.visible_count(now), 4);
        let snap = h.snapshot(now + Duration::from_millis(50));
        assert_eq!(snap.len(), 4);
        for g in &snap {
            assert!((g.progress - 0.5).abs() < 1e-3);
        }
    }

    #[test]
    fn default_matches_soft_green_preset() {
        let a = RevealOpts::default();
        let b = RevealOpts::soft_green();
        assert_eq!(a.char_interval, b.char_interval);
        assert_eq!(a.fade_duration, b.fade_duration);
        assert_eq!(a.fade_from, b.fade_from);
        assert_eq!(a.fade_to, b.fade_to);
    }
}
