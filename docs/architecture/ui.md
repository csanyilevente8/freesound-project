# UI

Status: **CLI shell + egui prototype.** The GUI framework is decided and
implemented — **egui/eframe** (ADR-0002) — behind the `gui` feature of
`daw-app`. The headless CLI remains the default front end.

## Two front ends, one `daw-app`

- **CLI** (`daw` binary, default) — builds/inspects/renders/plays projects from
  the terminal. Always available, no GUI deps.
- **GUI** (`daw-gui` binary, `--features gui`) — the egui prototype. Optional so
  the default build and CI never pull windowing/GPU dependencies.

```bash
# CLI (default)
cargo run -p daw-app --bin daw -- help

# GUI (opt-in)
cargo run -p daw-app --features gui --bin daw-gui -- path/to/project.json
```

## What the GUI prototype does (ADR-0002 gates)

A thin presenter over `Project` + `daw-commands` + `daw-audio-io`:

- **Arrangement timeline** — track lanes with clip rectangles, a moving
  playhead, and a dedicated **seek ruler** (click/drag to move the playhead).
- **Editing (all undoable, routed through `daw-commands`)**:
  - single-click a clip to **select**; double-click to **select + seek**
  - drag a clip **body** to **move** it (`MoveClip`)
  - drag a clip **edge** to **trim** start/end (`SetClipGeometry`)
  - **S** splits the selected clip at the playhead (one undo unit)
  - **Delete/Backspace** removes the selected clip (`RemoveClip`)
  - **Cmd/Ctrl+Z** / **Shift+Cmd/Ctrl+Z** undo/redo, also in the Edit menu
- **Transport bar** — Play/Pause (Space), Return-to-start, position readout,
  L/R peak meters. Pause/resume retains position.
- **Menu / File → Open** — native file dialog (`rfd`).
- **Accessibility** — keyboard focus with a visible ring, shortcuts, hover text
  on icon buttons, transport state shown as text (not color alone).
- **Dark-first theme** using the spec §7.1 tokens.

Edits route through the real command/undo machinery; the GUI never mutates the
project directly. After an edit, playback stops and the next Play re-renders
(the pre-rendered buffer is invalidated).

### Known limitation

The seek ruler is laid out above the track scroll area, so its tick marks are
slightly **misaligned** horizontally with the clip rectangles. Functionally the
ruler seeks correctly; the cosmetic alignment (and horizontal scroll/zoom
syncing) is deferred to when timeline zoom/scroll is implemented.

No egui type appears in any crate below `daw-app`; a future frontend swap (the
documented Slint fallback) would not touch the core.

## Not yet in the GUI

- Effect/automation editing in the window (clip move/trim/split/delete are
  done; effect-chain and automation UIs are future slices).
- Timeline **zoom** and horizontal **scroll** (and the ruler/lane alignment
  that comes with them), mixer view, piano roll, inspector, browser.
- Grid/bar-beat snapping for edits (currently frame-exact).
- A deeper screen-reader pass (AccessKit) beyond the keyboard/label basics.

The GUI deliberately stays a **presenter**: all edits route through
`daw-commands`, and all playback through `daw-audio-io`, so the window never
becomes the source of truth.
