# UI Implementation Plan — from the HTML mockup to the real GUI

This plan turns the design in `docs/design/ui-prototype.html` (kept in the
repo as the reference mock) into a concrete, prioritized build-out of the real
egui GUI (`daw-app`, `gui` feature, `daw-gui` binary).

Guiding rules (unchanged from the architecture):
- The GUI is a **presenter**. Every edit routes through `daw-commands` + `History`;
  playback/metering through `daw-audio-io`. No egui type leaks below `daw-app`.
- Keep the headless build/CI and the 219 tests unaffected (GUI stays
  feature-gated).
- Build in small, independently testable slices; keep it buildable after each.

Legend for "status":
- **Have** — backend exists; this is GUI wiring only.
- **Partial** — some backend exists; needs a bit more engine/model work.
- **Blocked:<milestone>** — needs a feature not yet built (named milestone).

---

## 1. Mockup region → backend mapping

### Top bar
| Element | Status | Backend / notes |
|---|---|---|
| Brand / project name / "Saved" | Have | `Project.name`; dirty flag from `History` depth vs last save |
| Save (↧) | Have | `daw_project::save` (atomic, versioned) |
| Settings (⚙) modal | Have | sample rate / tempo / time sig from `Project`; device info from `daw-audio-io` |
| "Audio engine ready" status dot | Have | derive from whether a device opened (`CpalOutput`) |

### Transport
| Element | Status | Backend / notes |
|---|---|---|
| Play / Pause / Stop | Have | `PlaybackHandle` Play/Stop + GUI pause-resume (done) |
| Time display (bars:beats:ticks) | Have | playhead frame → bars/beats via `MusicalTime` + tempo |
| BPM field | Have | `Project.tempo` (needs a `SetTempo` command — small add to `daw-commands`) |
| Time signature select | Have | `Project.time_signature` (+ `SetTimeSignature` command) |
| Loop toggle + loop range | Partial | model has no loop range yet; add `loop_range` to `Project` (schema v5) + engine honoring it |
| Record (●) arm | Blocked:P2 (recording) | needs capture pipeline |
| Master button | Partial | master track exists; a master strip UI is new |

### Library browser (left)
| Element | Status | Backend / notes |
|---|---|---|
| Panel + collapse + search + categories | Have (UI only) | pure egui; no backend needed for the shell |
| Samples list (real files) | Partial | `daw-media` imports WAV; needs a folder scan + list model |
| Instruments / Presets | Blocked:P4 (instruments) / later | no instrument/preset system yet |
| Effects list (draggable to chain) | Have (data) | the 8 processors exist in the engine factory; needs a registry + drag-drop |
| Drag asset/effect onto a track | Partial | drop target wiring is new GUI work; underlying add-clip/add-effect exist |

### Workspace toolbar
| Element | Status | Backend / notes |
|---|---|---|
| Toggle browser / detail panel | Have (UI only) | layout state |
| Snap toggle + grid size | Have | quantize edit positions to bar/beat via `MusicalTime`→`SampleTime` |
| Zoom in/out | Have | view-only (`px_per_sec`); fixes ruler/lane alignment when done properly |
| + Track | Have | `AddTrack` command |

### Arrangement (center)
| Element | Status | Backend / notes |
|---|---|---|
| Sticky ruler + bar labels | Have | ruler exists; proper alignment = the deferred shared-scroll fix |
| Sticky track gutter / headers | Have (UI) | current GUI has headers; sticky-left is layout work |
| Track rows: color, name, M/S/arm, type | Have | color field is new on `Track` (schema add); mute/solo/rename exist; arm = P2 |
| Clips: waveform render | Partial | `daw-media::compute_peaks` gives the data; draw it instead of a flat rect |
| Clips: MIDI note render | Blocked:P4 | needs MIDI clip model |
| Select / move / trim / split / delete | Have | done in the current GUI (via commands) |
| Double-click empty lane → create clip | Partial | needs an asset to reference; wire to browser selection or import |
| Playhead + click/drag ruler to seek | Have | done |

### Detail panel (bottom)
| Element | Status | Backend / notes |
|---|---|---|
| Track volume knob | Have | `SetTrackVolume` |
| Pan knob | Partial | model has no per-track pan field yet; add `pan` to `Track` + apply in render |
| Send A knob | Blocked (sends) | bus routing/sends not implemented |
| Effect chain list + bypass | Have | `EffectChain`, `SetEffectBypass`, add/remove/reorder commands |
| Add effect | Have | `AddEffect` |
| Effect parameter knobs | Have (data) | `SetEffectParam` exists; needs per-effect param UI |
| Output meter L/R + peak | Have | `Event::Meter { peak_l, peak_r }` from the player |
| "44.1 kHz · 128 samples · Stereo" | Have | device info from `daw-audio-io` |

### Footer / global
| Element | Status | Backend / notes |
|---|---|---|
| Status, track count, time sig | Have | from `Project` + state |
| Toasts | Have (UI) | status strings already exist |
| Modals (settings, save, confirm) | Have (UI) | egui modal/window |
| Keyboard (Space, Delete, Esc) | Have | already wired; extend per shortcuts |
| Responsive layout + light/dark | Have (UI) | egui supports both; tokens already in the GUI |
| Accessibility (focus, labels) | Have | prototype gate passed; keep extending |

---

## 2. Small backend additions this UI implies

These are the only non-GUI pieces the mockup needs beyond what exists. All are
small, offline-testable, and fit existing seams:

1. **Commands:** `SetTempo`, `SetTimeSignature`, `SetTrackPan`, `SetTrackSolo`
   (mute exists), `SetTrackColor`, `RenameTrack` (exists). Each ~a dozen lines +
   a test, mirroring existing commands.
2. **`Track.pan`** field + apply in `render_block` (there's already a
   `PanProcessor`); schema bump + migration (v4→v5).
3. **`Track.color`** field (cosmetic, serialized); schema add.
4. **Loop range** on `Project` + engine honoring it in playback (and the loop
   toggle). Schema add.
5. **Effect registry** for the browser: a list of `(type_id, display_name,
   default params)` the factory already knows — expose it so the UI can show
   "available effects".
6. **Media/folder listing** for the browser: scan a directory for importable
   WAVs (reuses `daw-media`).

Everything else is GUI composition.

---

## 3. Prioritized phases (each buildable + testable)

Ordered by value and by what's unblocked today.

### Phase A — "Real controls" (all **Have**, no new milestones) — ✅ DONE
Wire the panels the backend already supports; biggest visible jump, lowest risk.
- Detail panel: track **volume**, **mute/solo**, **effect chain list + bypass +
  remove**, **output meter**. ✅ (effect *add* from a registry → Phase C)
- Transport: **BPM** editing (`SetTempo`). ✅ (time-signature select → later)
- Top bar: **Save**, engine-ready indicator. ✅ (settings modal → later)
- **Live master volume** (`SetMasterGain`, no re-render). ✅
- Removing an effect cleans up its automation lanes (transaction). ✅
- Tests: `SetTrackSolo`/`SetTempo`/`SetTimeSignature`/`RemoveAutomationLane`
  command tests added. ✅

### Phase B — Arrangement polish (mostly **Have/Partial**)
- **Zoom** + proper **ruler/lane alignment** (the deferred shared-scroll-area
  fix) + horizontal scroll.
- **Snap-to-grid** for move/trim/split (quantize via `MusicalTime`).
- **Waveform** rendering in clips (`compute_peaks`).
- **Track color** + **pan** (small model additions + commands).
- Double-click empty lane to create a clip (wire to a selected asset).

### Phase C — Library browser (**Partial**)
- Panel shell + search + categories (UI).
- **Effects** category from the effect registry, **drag onto a track's chain**.
- **Samples** category from a folder scan, **drag onto a lane** → import + clip.
- Instruments/Presets shown but disabled until P4.

### Phase D — Loop + master (**Partial**)
- Loop range model + engine support + the loop UI.
- Master strip (volume/meter/insert chain on the master track — mostly reuses
  Phase A pieces).

### Phase E — Blocked features (land as their milestones ship)
- **Recording / arm** → when **P2 (recording)** is built: wire the ● arm, input
  selection, and record into a new clip.
- **MIDI tracks + note render + piano roll + instrument** → when **P4 (MIDI +
  synth)** is built.
- **Sends / buses** → when send routing is implemented.

---

## 4. What is NOT achievable without new foundations

Honest list — these are the only mockup elements with no current backbone, each
tied to a known future milestone (not a redesign):
- Live **recording** and the arm button (P2).
- **MIDI** clips/notes, the implied piano-roll, and software **instruments**
  (P4).
- **Sends** and real bus **routing** between tracks.
- A populated, drag-and-drop **content library** with presets/instruments
  (depends on import + instrument/preset systems).

Everything else in the mockup is reachable with the current architecture.

---

## 5. One structural note

The mockup's **live-edit-while-playing** feel (meters moving, playhead running
while you tweak) ultimately wants the **prepared-graph swap** (ADR-0007,
deferred): today an edit stops playback and the next Play re-renders. Phase A/B
work fine with the current "edit → re-render on next Play" model; the graph-swap
is a later real-time-engine upgrade, not a GUI blocker.

---

Reference mock: `docs/design/ui-prototype.html` (repo). This plan should
be revisited when P2 (recording) and P4 (MIDI/instruments) land, since they
unblock Phases D–E.
