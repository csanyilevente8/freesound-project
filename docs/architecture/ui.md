# UI

Status: **Minimal, framework-agnostic shell.** The GUI framework is decided —
**egui**, accepted pending a prototype (ADR-0002) — but not yet implemented; the
shell is still text-only.

## What exists (M0)

- `daw_app::AppState` — a small, UI-framework-independent application state
  holding the open `Project`, plus a `describe()` method that renders a
  human-readable summary.
- A `daw` binary that creates a new empty project, prints its summary, and
  optionally (`--save <path>`) saves and reloads it to demonstrate the
  versioned persistence round-trip.

Example output:

```
Project: Untitled
  id:            <uuid>
  sample rate:   48000 Hz
  tempo:         120 bpm
  time sig:      4/4
  tracks:        1
    [0] master "Master"

Status: no audio device configured (offline). Playback and recording are not
implemented in Milestone 0.
```

The shell explicitly states that playback/recording are not implemented — it
does not pretend to be a working DAW UI.

## Why no GUI yet

The offline slices prove the project model, serialization, DSP, effects, import,
and undo/redo without a GUI. The toolkit is now chosen — **egui** (ADR-0002),
accepted pending a prototype whose gates explicitly include an
accessibility/keyboard check — but implementation waits for the UI phase. The
framework-agnostic `AppState` + `daw-commands` boundary means the frontend
becomes a thin presenter and cannot leak toolkit types into the core crates; if
the egui accessibility gate fails, switching to Slint touches only the UI layer.

## Deferred (spec §7)

The entire graphical interface: main window regions (menu/transport/arrangement
/mixer/browser-inspector), the arrangement timeline, mixer channel strips, the
piano roll, effect editors, the browser, device preferences, dialogs, theming
(dark-first with the proposed tokens), and the accessibility pass. These arrive
alongside the features they present, starting at Milestone 1.
