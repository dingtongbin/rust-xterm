<p align="center">
  <h1 align="center">rust-xterm</h1>
  <p align="center">Industrial-grade terminal emulation library for Rust — built on WezTerm-term</p>
  <p align="center">
    <a href="https://github.com/dingtongbin/rust-xterm/actions"><img alt="CI" src="https://github.com/dingtongbin/rust-xterm/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://docs.rs/rust-xterm-core"><img alt="docs.rs" src="https://docs.rs/rust-xterm-core/badge.svg"></a>
    <img alt="Rust 1.88+" src="https://img.shields.io/badge/rust-1.88%2B-orange.svg">
    <img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg">
    <img alt="unsafe forbidden" src="https://img.shields.io/badge/unsafe-forbidden-success.svg">
  </p>
</p>

---

## Overview

`rust-xterm` is a terminal emulation library that brings the maturity of [WezTerm](https://wezfurlong.org/wezterm/)'s VT state machine to any Rust GUI framework. It is **not** a terminal emulator application — it is the embeddable engine that powers one.

### Design philosophy

- **Static determinism** — All memory models are locked at initialization; zero runtime allocation on the hot path.
- **Absolute decoupling** — The core library has zero OS/GUI dependencies and can be embedded into Slint, winit, egui, Tauri, or a headless test harness.
- **Zero-overhead abstraction** — The anti-corruption layer around `wezterm_term::Terminal` forwards references; no extra copies.
- **Memory safety** — `#![forbid(unsafe_code)]` on every crate root. Zero `unsafe` blocks across the entire codebase.

## Architecture

```
rust-xterm/
├── crates/
│   ├── rust-xterm-core/       # VT state machine + codec + damage tracker (no OS/GUI deps)
│   ├── rust-xterm-renderer/   # Swash rasterization + texture atlas + font tree
│   └── rust-xterm-host/       # PTY bridge + event loop skeleton
├── examples/              # Runnable integration examples
├── benches/               # Criterion benchmarks
└── Cargo.toml             # Workspace + version lock
```

## Quick start

```rust
use rust_xterm_core::{TerminalManager, TerminalSize};
use std::time::Instant;

let mut term = TerminalManager::utf8(TerminalSize::new(24, 80));
term.write(b"\x1b[31mHello, \x1b[1mrust-xterm\x1b[0m!\n");

if let Some(frame) = term.poll_frame(Instant::now()) {
    for rect in &frame.dirty_rects {
        println!("repaint {:?}", rect);
    }
}
```

## xterm.js-style API

rust-xterm mirrors the familiar xterm.js surface so frontend engineers feel at home:

| xterm.js                | rust-xterm-core                          |
|-------------------------|--------------------------------------|
| `new Terminal(opts)`    | `TerminalManager::utf8(size)`        |
| `term.write(data)`      | `term.write(bytes)`                  |
| `term.onResize(cb)`     | `term.resize(size)` (poll-based)     |
| `term.buffer.active`    | `term.screen_snapshot()`             |
| `term.registerMarker`   | `Marker` (seqno-based)               |
| `term.parser.register`  | `Parser` extension trait             |
| `term.loadAddon(addon)` | `Addon` trait + `Terminal::load_addon` |

## Features

- **Encodings**: UTF-8 passthrough, GBK, Big5, Shift_JIS, EUC-KR with split-packet handling.
- **Colors**: 16-color, 256-color, 24-bit true color, default Campbell palette (Windows Terminal).
- **Styles**: bold, dim, italic, underline, double-underline, undercurl (sine-wave), strikethrough, blink, reverse, invisible.
- **Cursor**: block / bar / underline, blinking, visibility tracking.
- **Scrollback**: configurable, bounded ring buffer (no unbounded growth).
- **Mouse**: SGR mouse mode, button/ motion tracking.
- **Hyperlinks**: OSC 8 hyperlink support.
- **CWD report**: OSC 7 `file://` URL parsing emits `CwdChange(PathBuf)` event.
- **Focus report**: DECSET 1004 — `set_focused` generates `\x1b[I` / `\x1b[O` to `drain_output`.
- **Scroll region**: DECSTBM queryable via `scroll_region() -> Option<(usize, usize)>`.
- **Keyboard mapping**: `KeyInput` enum + `KeyMapping::encode_key` (SS3/CSI dual mode, Ctrl/Alt, F1–F12).
- **Selection**: linear + rectangular `SelectionRange`, mouse-driven (single/double/triple click, drag), `selection_text()` extraction, `SelectionReady` auto-copy event.
- **Synchronized output**: DECSET 2026 batched repaint.
- **Bracketed paste**: DECSET 2004.
- **Alternate screen**: DECSET 1049.
- **CJK**: wide-character width detection, proper double-cell rendering.
- **Emoji**: color-glyph fast path via `swash` ColorOutline / ColorBitmap.
- **Ligatures**: GSUB/GPOS shaping via `swash::shape` (toggleable via `RendererConfig::enable_ligatures`).
- **Smart selection**: double-click detects URLs (`http://`/`https://`/`ftp://`/`file://`/`ssh://`), Unix paths, IPv4/IPv6 — no `regex`/`url` dependency.
- **Rectangular block selection**: Alt+drag for column-aligned selection.
- **IME pre-edit**: `set_preedit` / `commit_text` / `clear_preedit` API + `PreeditChange` event; renderer draws underlined pre-edit text at cursor.
- **Unicode grapheme clusters**: `unicode-segmentation` based selection boundary — ZWJ sequences, flag pairs, skin-tone modifiers stay intact.
- **Sixel images**: built-in DCS `q` parser (RLE + color registers) → RGBA blit.
- **iTerm2 inline images**: OSC 1337 `File=inline=1` parser with PNG/JPEG decoding.
- **Global glyph cache**: `GlobalAtlas` (`OnceLock<Arc<Mutex<TextureAtlas>>>`) shares glyphs across `Renderer` instances.
- **Sub-row dirty tracking**: `mark_span_dirty(row, col_start, col_end)` + `DirtySpan` + `render_row_segment` for column-level repaint.

## GUI demos

Three independent demo packages under `demos/` — each is its own Cargo workspace root with its own `Cargo.lock`, so GUI framework dependencies never leak into the core workspace:

| Demo | Framework | Features |
|------|-----------|----------|
| `demos/slint-demo` | Slint 1.6 | Image widget, Timer-driven tick, keyboard/mouse/resize, FPS+mem status bar |
| `demos/iced-demo` | iced 0.13 | Subscription-based tick, wheel scrollback, full keyboard mapping |
| `demos/egui-demo` | eframe 0.29 | TextureHandle upload, request_repaint, wheel scrollback |

Verify isolation: `grep -E 'name = "(slint\|iced\|egui\|eframe)"' /workspace/Cargo.lock` returns nothing.

## Integration

rust-xterm-core has **zero** GUI dependencies. To embed it:

1. Drive `TerminalManager::write` from your PTY / SSH source.
2. Poll `TerminalManager::poll_frame` on a 60 fps timer.
3. Push `frame.dirty_spans` into your GPU texture / Skia / Slint image.
4. Forward user keystrokes via `TerminalManager::write` (encoded) or your PTY bridge.

See `examples/` for a headless demo and `crates/rust-xterm-host` for a `portable-pty` bridge.

## Safety & performance guarantees

| Guarantee                | How it is enforced                                             |
|--------------------------|----------------------------------------------------------------|
| No `unsafe`              | `#![forbid(unsafe_code)]` on all crate roots + CI grep check   |
| Rust 1.88 strict lock    | `rust-version = "1.88"` in workspace root + CI `+1.88.0`     |
| Bounded memory           | Fixed-size atlas, bounded scrollback, LRU eviction             |
| 0% CPU at idle           | `poll_frame` returns `None` when no damage and no blink due    |
| No data races            | Single-owner `&mut self` model, no `Arc<Mutex>` on hot path    |

## License

Apache-2.0. See [LICENSE](LICENSE).
