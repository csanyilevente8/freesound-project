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

A thin presenter over `Project` + `daw-audio-io`:

- **Arrangement timeline** — track lanes with clip rectangles and a moving
  playhead.
- **Transport bar** — Play/Pause (Space), Return-to-start, position readout,
  L/R peak meters. Pause/resume retains position (the real-time `Player` keeps
  its sample position across Stop).
- **Menu / File → Open** — native file dialog (`rfd`).
- **Accessibility** — keyboard focus with a visible ring, Space shortcut, hover
  text on icon buttons, and transport state shown as text (not color alone).
- **Dark-first theme** using the spec §7.1 tokens.

No egui type appears in any crate below `daw-app`; a future frontend swap (the
documented Slint fallback) would not touch the core.

## Not yet in the GUI

- Editing in the window (drag clips, edit effects/automation) — the
  `daw-commands` layer exists and will be wired to the timeline next.
- Click-to-seek / scrub, zoom controls, mixer view, piano roll, inspector,
  browser — later slices.
- A deeper screen-reader pass (AccessKit) beyond the keyboard/label basics.

The GUI deliberately stays a **presenter**: all edits will route through
`daw-commands`, and all playback through `daw-audio-io`, so the window never
becomes the source of truth.
