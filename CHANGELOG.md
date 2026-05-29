# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] — 2026-05-29

### Added

- Per-grapheme sound (`--sound <PATH|URL>`): plays a short sound
  (best-effort) each time a reveal frame brings new non-whitespace
  graphemes into view, giving the typewriter a voice (typewriter clack,
  Dragon-Quest-style text beep, etc.). Works in reader mode (`--read`) too.
  - The `jiwa` binary stays **zero-dependency**: jiwa never decodes audio.
    The sound *file* is user-supplied; playback shells out to an OS player
    (`ffplay` / `mpv` / `aplay` / `pw-cat`, or macOS `afplay`), and URLs are
    fetched once via `curl` (or `wget`). No audio or HTTP crate is added;
    the library crate is untouched.
  - File/network I/O happens **once at load**: bytes are held in memory and
    replayed (graphemes never re-read or re-download). A URL is cached under
    `$XDG_RUNTIME_DIR`/`$TMPDIR`/`/tmp` keyed by a SipHash of the URL and
    reused if present; cleanup is left to the OS (jiwa holds no state and
    leaves no resident player — each play spawns a fresh process and never
    waits, so the reveal is never blocked).
  - Fully **best-effort**: a missing file, missing fetcher, missing player,
    or failed playback is silent (at most one quiet stderr note at startup);
    the reveal always runs. WAV is recommended (plays from stdin cleanly).
- Interactive reader (sound-novel) mode (`--read`): reveal a piped novel
  one segment at a time, pausing for Enter between segments — the terminal
  equivalent of a visual-novel "click to continue". Phase 1, dependency-free
  (no raw mode).
  - `--by <UNIT>` chooses the segment unit: `sentence` (default, splits on
    Japanese 。！？ + trailing close brackets and on English `. ! ?`
    followed by whitespace/end, leaving decimals like `3.14` intact),
    `paragraph` (blank-line delimited), or `line`.
  - Keypresses are read from the controlling terminal (`/dev/tty`) because
    stdin holds the novel — the `less` / `fzf` / `git add -p` pattern.
    Enter advances; `q` or Ctrl-D (EOF) ends the session.
  - The waiting prompt is erased on advance, so scrollback keeps only the
    novel text. Each segment uses the existing reveal flags
    (`--fade` / `--stagger` / `--from` / `--to` / `--fps`).
  - Pipe-safe: non-TTY stdout (or an unopenable `/dev/tty`) falls back to
    verbatim passthrough. The library crate stays unchanged.

## [0.2.0] — 2026-05-29

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
