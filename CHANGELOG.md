# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] — 2026-05-17

Initial release. Extracted from `type-globe`'s in-tree `jiwa_core` module
(designed to be liftable since 2026-04-30).

### Added

- `RevealHandle` / `RevealOpts` — typewriter reveal with per-grapheme fade-in.
  Unicode-segmentation-aware (handles combining marks and ZWJ emoji as
  single graphemes).
- `PulseHandle` / `PulseOpts` — sinusoidal dim ↔ bright "breathing"
  animation anchored to a single symbol.
- `Rgb(u8, u8, u8)` + `lerp_rgb(a, b, t)` for the renderer-agnostic
  color surface.
- Two presets to make the common cases one-liners:
  - `RevealOpts::soft_green()` — slightly slow typewriter, gray → soft
    green, comfortable for problem/quiz text on a dark terminal.
  - `PulseOpts::cyan_breath()` — ~1.5 s breath cycle from muted teal
    to bright cyan, designed for the listening "♪ now playing" affordance.
