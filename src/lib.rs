//! Terminal text **reveal** animations.
//!
//! Two small primitives, both deliberately renderer-agnostic:
//!
//! - [`RevealHandle`] — typewriter that appears one grapheme at a time,
//!   each grapheme fading from one [`Rgb`] to another over a configurable
//!   window.
//! - [`PulseHandle`] — a single symbol that "breathes" between two
//!   colors on a sinusoidal cycle.
//!
//! Both handles are **pure**: no I/O, no thread, no global clock. Every
//! time-bearing entry point takes an explicit [`std::time::Instant`] so
//! tests can step time deterministically and so the caller controls when
//! frames are emitted. You snapshot each animation once per redraw at
//! whatever cadence your renderer already runs (a TUI event loop, a
//! `crossterm` poll, etc.) and map the returned [`Rgb`] values to your
//! renderer's color type.
//!
//! # Example
//!
//! ```
//! use std::time::{Duration, Instant};
//! use jiwa::{RevealHandle, RevealOpts};
//!
//! let start = Instant::now();
//! let reveal = RevealHandle::start_at("Hello, 世界", RevealOpts::default(), start);
//!
//! // Render at any later instant — the caller chooses the cadence.
//! let frame = reveal.snapshot(start + Duration::from_millis(100));
//! for g in &frame {
//!     // `g.text` is the grapheme cluster to draw, `g.color` is the
//!     // RGB to draw it in, `g.progress` is the 0..=1 fade progress.
//!     print!("{}", g.text);
//! }
//! ```
//!
//! # Why "jiwa"
//!
//! Japanese onomatopoeia for something appearing slowly, "じわじわ" —
//! the visual feel the typewriter+fade combination is going for.
//!
//! # Renderer mapping
//!
//! [`Rgb`] is intentionally not a wrapper around `crossterm::style::Color`
//! or `ratatui::style::Color` — pulling either in would force every
//! downstream caller to agree on the same renderer. Map at the call
//! site instead:
//!
//! ```ignore
//! use ratatui::style::Color;
//! let g = reveal.snapshot(now);
//! let color = Color::Rgb(g[0].color.0, g[0].color.1, g[0].color.2);
//! ```

pub mod color;
pub mod pulse;
pub mod reveal;

pub use color::{lerp_rgb, Rgb};
pub use pulse::{PulseFrame, PulseHandle, PulseOpts};
pub use reveal::{RevealHandle, RevealOpts, RevealedGrapheme};
