# ADR-0002: GUI framework

**Status:** Accepted pending prototype — **egui/eframe** is the chosen default,
to be confirmed once a time-boxed prototype passes the gates below. Implementation
is not started (we remain offline until the UI/playback phase).

## Context

The DAW needs a cross-platform GUI (Windows/macOS/Linux) with docking/resizing
panels, high-DPI rendering, keyboard focus/accessibility, native file dialogs,
and good performance for a dense arrangement/mixer view (spec §7). Committing to
a toolkit prematurely risks coupling the core to it, which the spec forbids
(§3.1, §4.3). All project, command, and audio logic is already independent of
any GUI and must stay that way (a future frontend is a presenter over
`daw_app::AppState` + the `daw-commands` layer).

## Decision

Adopt **egui / eframe** as the GUI framework, **conditionally**: the decision is
confirmed only if a focused prototype passes the acceptance gates below. If it
fails on accessibility or input/text behavior, evaluate **Slint** before
finalizing. Either way, no GUI type may appear in `daw-core`, `daw-project`,
`daw-engine`, `daw-dsp`, `daw-media`, or `daw-commands`.

### Why egui is the default

- Immediate-mode rendering is a strong fit for a **custom-drawn, zoomable
  arrangement timeline** — our most unusual UI surface — so the timeline is
  expected to be the *easy* part, not the risk.
- Pure-Rust, easy high-DPI, simple to embed, actively maintained, permissive
  license (MIT/Apache-2.0, GPL-compatible per ADR-0001).
- Docking via `egui_dock`.

### Known risks (what the prototype must de-risk)

- **Accessibility is egui's structural weakness.** Immediate-mode has no
  persistent widget tree for platform accessibility APIs; `AccessKit` provides
  partial coverage (weaker on Linux). Spec §7.12 makes keyboard focus,
  accessible names for icon-only controls, and no-color-only state **hard
  requirements**, so this is the most likely reason egui fails the gate — more
  than docking or the timeline.
- Native menus and some text-input/IME behaviors are less mature than in
  retained/native toolkits.

## Prototype acceptance gates

The prototype is time-boxed and must demonstrate, on at least macOS + one of
Windows/Linux:

1. **Custom timeline:** draw tracks + clips on a zoomable/scrollable canvas,
   select and drag a clip (routing the edit through `daw-commands`), at
   interactive frame rates with a few dozen clips.
2. **Panel layout:** resizable/collapsible docked panels (arrangement + a second
   panel) whose layout persists across runs.
3. **Accessibility/keyboard (gating):** full keyboard navigation with a visible
   focus indicator; icon-only buttons expose accessible names; at least one
   state (e.g. record/clip-selected) is conveyed by more than color. If this
   gate cannot be met at a basic level, egui is rejected and Slint is evaluated.
4. **Native file dialog:** open/save dialog works on each target OS (via `rfd`
   or equivalent) — does not have to be egui-native.
5. **High-DPI:** legible at 1x and 2x scaling.

Record the prototype result (pass/fail per gate) as a short note appended here
when the UI phase begins.

## Consequences

- A thin `daw-app` presenter layer will sit over `AppState` + `daw-commands`;
  core crates stay GUI-free (enforced by crate boundaries, already in place).
- If egui is rejected on accessibility, the switch to Slint touches only
  `daw-app` and a new UI crate — no core change — because the GUI is already
  behind the application boundary.
- `egui_dock`, `rfd` (file dialogs), and `AccessKit` would be new dependencies
  evaluated for license/platform at adoption time and added to the inventory.

## Revisit when

Starting the UI phase (run the prototype), or if an accessibility gate fails
(evaluate Slint).
