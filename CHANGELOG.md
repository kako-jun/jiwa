# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- `jiwa` CLI binary: pipe text in (`echo "Hello" | jiwa --fade 200ms`),
  watch it reveal on a TTY. Dependency-free — raw ANSI control, TTY
  detection via `std::io::IsTerminal`. Flags: `--fade`, `--stagger`,
  `--from`, `--to`, `--fps`, `-h/--help`, `-V/--version`.
  - Pipe-safe: non-TTY (or no animation requested) passes input through
    verbatim, so `jiwa ... | other` and `jiwa ... > file` stay clean.
  - Passes through existing ANSI color in the input (e.g. `lolcat` output),
    using the reveal only for timing.
  - Reuses the library `RevealHandle` engine; the library stays
    renderer-agnostic and dependency-free.
  - Value-taking flags accept both `--fade 200ms` and `--fade=200ms`.
  - While animating, line-wrap is disabled and frames redraw in place;
    the final confirmed render re-enables wrap so long lines wrap in
    scrollback. No signal handler is installed (dependency-free), so an
    interrupted animation may leave the cursor hidden / wrap off until
    `reset`.

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
