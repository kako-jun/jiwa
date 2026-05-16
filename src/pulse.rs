//! Single-symbol "breathing" animation.
//!
//! While something is in flight (audio playing, a job running, a
//! waiting prompt) the [`PulseHandle`] cycles a single symbol between a
//! dim and bright color on a sinusoidal cadence — visually a calm
//! breath rather than a hard blink.
//!
//! Same design constraints as [`crate::reveal`]:
//! - **Pure**: no I/O, no thread, no global clock. Every entry point
//!   takes an explicit `Instant`.
//! - **Renderer-agnostic**: returns an [`Rgb`] triple for the caller
//!   to map to whatever color type its renderer uses.
//!
//! The pulse uses a sinusoidal waveform anchored so the symbol *starts*
//! at `color_dim` (progress 0) and reaches `color_bright`
//! (progress 1) at the half-period mark.

use std::time::{Duration, Instant};

use crate::color::{lerp_rgb, Rgb};

/// One frame of a pulse at a given instant.
#[derive(Debug, Clone, PartialEq)]
pub struct PulseFrame {
    /// Symbol the renderer should draw (typically `"♪"`).
    pub text: String,
    /// Foreground color at the snapshot time.
    pub color: Rgb,
    /// Linear progress in `[0.0, 1.0]` along the dim→bright axis.
    /// 0.0 = `color_dim`, 1.0 = `color_bright`. The wave is symmetric
    /// so this value rises and falls smoothly each period.
    pub progress: f32,
}

/// Tunables for one pulse animation.
#[derive(Debug, Clone, Copy)]
pub struct PulseOpts {
    /// One full cycle: dim → bright → back to dim. Zero is treated as
    /// "static at the bright end" so callers can disable the pulse
    /// without special-casing.
    pub period: Duration,
    pub color_dim: Rgb,
    pub color_bright: Rgb,
}

impl PulseOpts {
    /// ~1.5 s breath cycle from a muted teal to a bright cyan. Designed
    /// for a `♪` "now playing" indicator on a dark terminal.
    pub const fn cyan_breath() -> Self {
        Self {
            period: Duration::from_millis(1500),
            color_dim: Rgb(40, 60, 80),
            color_bright: Rgb(80, 200, 255),
        }
    }
}

impl Default for PulseOpts {
    /// Same as [`PulseOpts::cyan_breath`].
    fn default() -> Self {
        Self::cyan_breath()
    }
}

/// One in-flight pulse. Cheap to construct; tick by calling
/// [`PulseHandle::snapshot`] each frame at whatever cadence the
/// renderer already runs.
#[derive(Debug)]
pub struct PulseHandle {
    text: String,
    started_at: Instant,
    opts: PulseOpts,
}

impl PulseHandle {
    /// Anchor the pulse at `now`. Tests pass an explicit `now` to step
    /// time without sleeping; production callers use [`PulseHandle::start`].
    pub fn start_at(text: &str, opts: PulseOpts, now: Instant) -> Self {
        Self {
            text: text.to_string(),
            started_at: now,
            opts,
        }
    }

    /// Convenience for production callers: anchors at `Instant::now()`.
    pub fn start(text: &str, opts: PulseOpts) -> Self {
        Self::start_at(text, opts, Instant::now())
    }

    /// Snapshot the pulse at `now`. Always returns one frame because the
    /// pulse is tied to a single symbol — there is no "hidden until later"
    /// state like the typewriter has.
    pub fn snapshot(&self, now: Instant) -> PulseFrame {
        let elapsed = now.saturating_duration_since(self.started_at);
        let progress = pulse_progress(elapsed, self.opts.period);
        let color = lerp_rgb(self.opts.color_dim, self.opts.color_bright, progress);
        PulseFrame {
            text: self.text.clone(),
            color,
            progress,
        }
    }
}

/// Sinusoidal wave anchored so progress(0) = 0 and progress(period/2) = 1.
/// The wave is `sin(2π t/T − π/2)` shifted into `[0,1]`.
fn pulse_progress(elapsed: Duration, period: Duration) -> f32 {
    if period.is_zero() {
        return 1.0;
    }
    let t = elapsed.as_secs_f64() / period.as_secs_f64();
    let phase = (t * std::f64::consts::TAU) - std::f64::consts::FRAC_PI_2;
    (((1.0 + phase.sin()) / 2.0) as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> PulseOpts {
        PulseOpts {
            period: Duration::from_millis(1000),
            color_dim: Rgb(0, 0, 0),
            color_bright: Rgb(200, 200, 200),
        }
    }

    fn epoch() -> Instant {
        Instant::now()
    }

    #[test]
    fn progress_is_zero_at_t_zero() {
        let now = epoch();
        let h = PulseHandle::start_at("♪", opts(), now);
        let frame = h.snapshot(now);
        assert!(frame.progress.abs() < 1e-3);
        assert_eq!(frame.color, Rgb(0, 0, 0));
        assert_eq!(frame.text, "♪");
    }

    #[test]
    fn progress_peaks_at_half_period() {
        let now = epoch();
        let h = PulseHandle::start_at("♪", opts(), now);
        let frame = h.snapshot(now + Duration::from_millis(500));
        assert!((frame.progress - 1.0).abs() < 1e-3);
        assert_eq!(frame.color, Rgb(200, 200, 200));
    }

    #[test]
    fn progress_returns_to_zero_at_full_period() {
        let now = epoch();
        let h = PulseHandle::start_at("♪", opts(), now);
        let frame = h.snapshot(now + Duration::from_millis(1000));
        assert!(frame.progress.abs() < 1e-3);
    }

    #[test]
    fn quarter_period_is_midway() {
        let now = epoch();
        let h = PulseHandle::start_at("♪", opts(), now);
        let frame = h.snapshot(now + Duration::from_millis(250));
        assert!((frame.progress - 0.5).abs() < 1e-3);
        assert_eq!(frame.color, Rgb(100, 100, 100));
    }

    #[test]
    fn pulse_repeats_each_period() {
        let now = epoch();
        let h = PulseHandle::start_at("♪", opts(), now);
        let a = h.snapshot(now + Duration::from_millis(300));
        let b = h.snapshot(now + Duration::from_millis(1300));
        assert!((a.progress - b.progress).abs() < 1e-3);
        assert_eq!(a.color, b.color);
    }

    #[test]
    fn zero_period_pins_at_bright() {
        let mut o = opts();
        o.period = Duration::ZERO;
        let now = epoch();
        let h = PulseHandle::start_at("♪", o, now);
        let frame = h.snapshot(now);
        assert_eq!(frame.progress, 1.0);
        assert_eq!(frame.color, o.color_bright);
    }

    #[test]
    fn arbitrary_text_is_passed_through() {
        let now = epoch();
        let h = PulseHandle::start_at("●", opts(), now);
        assert_eq!(h.snapshot(now).text, "●");
    }

    #[test]
    fn default_matches_cyan_breath_preset() {
        let a = PulseOpts::default();
        let b = PulseOpts::cyan_breath();
        assert_eq!(a.period, b.period);
        assert_eq!(a.color_dim, b.color_dim);
        assert_eq!(a.color_bright, b.color_bright);
    }
}
