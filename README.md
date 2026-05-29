# jiwa

[![crates.io](https://img.shields.io/crates/v/jiwa.svg)](https://crates.io/crates/jiwa)
[![docs.rs](https://img.shields.io/docsrs/jiwa)](https://docs.rs/jiwa)
[![CI](https://github.com/kako-jun/jiwa/actions/workflows/ci.yml/badge.svg)](https://github.com/kako-jun/jiwa/actions/workflows/ci.yml)

Terminal text **reveal** animations for Rust. Two small primitives:

- **Typewriter + per-grapheme fade-in** — characters appear one at a
  time and bloom from one color into another.
- **Pulse** — a single symbol "breathes" between two colors on a
  sinusoidal cycle, useful as a "now playing" / "thinking" indicator.

Both primitives are **renderer-agnostic**: they return plain `Rgb(u8, u8, u8)`
triples plus the text to draw, and you map those to whatever your
renderer already uses (`crossterm`, `ratatui`, raw ANSI, etc.). They are
also **pure** — every time-bearing call takes an explicit
`std::time::Instant`, so there is no global clock, no spawned thread,
and tests can advance time without sleeping.

The name is from the Japanese onomatopoeia **「じわじわ」** — something
appearing slowly, blooming into view.

## Install

```toml
[dependencies]
jiwa = "0.1"
```

## Example: typewriter reveal

```rust
use std::time::{Duration, Instant};
use jiwa::{RevealHandle, RevealOpts};

let start = Instant::now();
let reveal = RevealHandle::start_at("Hello, 世界", RevealOpts::default(), start);

// Tick once per redraw — your event loop chooses the cadence.
let frame = reveal.snapshot(start + Duration::from_millis(100));
for g in &frame {
    // g.text:    the grapheme cluster to draw
    // g.color:   Rgb(u8, u8, u8) to draw it in
    // g.progress: 0.0 (just appeared) .. 1.0 (fade complete)
    print!("{}", g.text);
}
```

The default preset (`RevealOpts::soft_green`) is tuned for problem /
quiz text on a dark terminal — 45 ms typewriter step, 320 ms fade from
a deep gray to a soft green so the in-between color reads as an
afterimage rather than a blink.

You can also disable either dimension by zeroing it:

- `char_interval = 0` → whole block fades in together (no typewriter).
- `fade_duration = 0` → pure typewriter, each grapheme appears at its
  final color.

## Example: pulse

```rust
use jiwa::{PulseHandle, PulseOpts};

let pulse = PulseHandle::start("♪", PulseOpts::default());
let frame = pulse.snapshot(std::time::Instant::now());
// frame.text == "♪"
// frame.color cycles between PulseOpts::color_dim and color_bright.
```

The default preset (`PulseOpts::cyan_breath`) gives a ~1.5 s breath
cycle from a muted teal to a bright cyan, designed for the "♪ audio
playing" affordance.

## CLI

`jiwa` also ships a tiny dependency-free binary so the same reveal engine
works from a shell pipe:

```sh
# whole block fades in
echo "Hello" | jiwa --fade 200ms

# typewriter
echo "Hello" | jiwa --stagger 50ms

# both, with custom colors
echo "Hello" | jiwa --fade 200ms --stagger 50ms --from "#444" --to "#fff"

# value flags also take the `=`-joined form
echo "Hello" | jiwa --fade=200ms --stagger=50ms

# pass-through: existing ANSI color is preserved, reveal is timing only
cat story.txt | lolcat | jiwa --stagger 30ms
```

When stdout is not a terminal (or no animation flag is given) the input is
passed through verbatim, so `jiwa ... | other` and `jiwa ... > file` stay
free of cursor-control noise. Run `jiwa --help` for the full flag list.

### Reader mode (sound novel)

`--read` turns a piped novel into an interactive reader: each segment
reveals, then `jiwa` waits for you to press **Enter** before moving on —
the terminal version of a visual-novel "click to continue".

```sh
# one sentence at a time (the default), with a gentle typewriter
cat novel.txt | jiwa --read --stagger 40ms

# one paragraph per Enter (paragraphs are split on blank lines — a line
# that is just a newline; a line with only spaces does not start a new one)
cat novel.txt | jiwa --read --by paragraph

# one line per Enter
cat lyrics.txt | jiwa --read --by line --fade 300ms
```

`--by` chooses the segment unit: `sentence` (default), `paragraph`, or
`line`. The reveal flags (`--fade` / `--stagger` / `--from` / `--to` /
`--fps`) apply to each segment.

Because the novel occupies stdin, keypresses are read from the
controlling terminal (`/dev/tty`) — the same approach `less`, `fzf`, and
`git add -p` use. Press Enter to advance; `q` or Ctrl-D ends the session.
The waiting prompt is erased once you advance, so the scrollback keeps
only the novel text. When stdout is not a TTY (or `/dev/tty` cannot be
opened), reader mode falls back to verbatim passthrough so pipes and
files stay clean. This is **Phase 1**: Enter-delimited and dependency-free
(no raw mode); single-keypress advance and reveal-skipping are future work.

### Long lines and interrupts

While animating, `jiwa` redraws each frame in place with line-wrap
disabled, so lines wider than the terminal are clipped during the
animation; the **final confirmed render re-enables line-wrap** before
drawing, so the permanent output in your scrollback wraps long lines
normally. This two-stage approach keeps the in-place animation from
scrolling the terminal while still leaving correct, wrapped text behind.

`jiwa` is intentionally dependency-free and installs **no signal
handler**. If you interrupt an animation with Ctrl-C, the terminal can be
left with the cursor hidden and line-wrap turned off. Run `reset` to
restore it.

## Mapping to your renderer

`jiwa::Rgb` is intentionally not a wrapper around `crossterm::style::Color`
or `ratatui::style::Color`. Map at the call site:

```rust
// ratatui
let color = ratatui::style::Color::Rgb(g.color.0, g.color.1, g.color.2);

// crossterm
let color = crossterm::style::Color::Rgb { r: g.color.0, g: g.color.1, b: g.color.2 };
```

## Concurrency note

These primitives are pure functions of `(time, text, opts)`. They do
not own a thread, do not poll for input, and do not sleep. Anything
your reveal-in-progress UI needs to do concurrently (accept input
during the reveal, abort early, restart) is the responsibility of the
surrounding event loop — `jiwa` just answers "what does the frame look
like right now?".

## Status

- **v0.1.0** — library primitives extracted from
  [type-globe](https://github.com/kako-jun/type-globe)'s in-tree
  `jiwa_core` module (where they have been used in production since
  v0.6.0).
- **CLI binary** — `jiwa` reads stdin and reveals it on stdout
  (`echo "Hello" | jiwa --fade 200ms`). Dependency-free, pipe-safe. See
  the [CLI](#cli) section above.

## Inspiration

The CLI direction takes cues from
[TerminalTextEffects](https://github.com/ChrisBuilds/terminaltexteffects)
(Python). `jiwa` is intentionally narrower — reveal-shaped effects
only, Unix-pipe friendly, Rust single binary.

## License

MIT
