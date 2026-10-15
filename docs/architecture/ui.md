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
- **Transport bar** — Play/Pause (Space), **● Record** (captures the input onto
  the selected track at the playhead), Return-to-start, position readout
  (seconds + bars:beats), an editable **BPM** field, and L/R peak meters.
  Pause/resume retains position.
- **Detail panel (bottom)** — for the selected track: a **volume** slider
  (dB-labeled), **Mute**/**Solo**, the **effect chain** (per-effect Bypass and
  remove), and the **output meter**. Plus a **live master volume** slider.
  Track controls route through `daw-commands` (undoable); master volume is a
  live output control sent to the player (`SetMasterGain`).
- **Menu / File → Open / Save** — native file dialog (`rfd`); versioned save.
- **Accessibility** — keyboard focus with a visible ring, shortcuts, hover text
  on icon buttons, transport state shown as text (not color alone).
- **Dark-first theme** using the spec §7.1 tokens.

Edits route through the real command/undo machinery; the GUI never mutates the
project directly.

### What is live vs. applied-on-next-play

- **Live (no re-render):** master volume (applied in the player), transport,
  seek, meters.
- **Applied on next Play (baked into the pre-rendered buffer):** per-track
  volume, mute/solo, effect bypass, and clip edits. Editing these while playing
  no longer restarts playback — the change is recorded and heard on the next
  Play. Making per-track processing fully live needs the prepared-graph swap
  (ADR-0007), deferred.
- **No audible effect yet:** BPM — clips are positioned in sample frames, so
  tempo only drives the bars:beats readout (it is still an undoable edit).
  Tempo-relative scheduling is a separate future slice.

Removing an effect also removes any automation lanes that target it, atomically
and undoably (otherwise the project would be invalid).

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

## Target design & build-out plan

A visual design mockup for the full interface lives at
`docs/design/ui-prototype.html` (open it in a browser). The concrete,
prioritized plan to build it on the existing architecture — mapping each UI
region to crates/commands and flagging which milestone unblocks each part — is
in `docs/ui-implementation-plan.md`.
