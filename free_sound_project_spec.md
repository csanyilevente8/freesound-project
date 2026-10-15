# Free Sound --- Product and Technical Specification

**Document type:** Product requirements + architecture specification\
**Status:** Initial project specification; decisions marked "proposed"
should be validated before implementation\
**Target platforms:** Windows, macOS, Linux\
**Primary implementation language:** Rust\
**Product model:** Free and open-source desktop digital audio
workstation (DAW)\
**Working name:** Free Sound (placeholder; do not hard-code the name into
architecture)

------------------------------------------------------------------------

## 1. Instructions

Use this document as the product direction and architectural baseline
for the project. Do not attempt to implement the entire DAW in one step.

1.  First inspect the repository, identify existing code and
    conventions, and summarize the current state.
2.  Convert this specification into a set of small, dependency-ordered
    implementation tasks or specs.
3.  Before major architectural decisions, record an Architecture
    Decision Record (ADR) with alternatives, trade-offs, and the chosen
    approach.
4.  Implement a thin, working vertical slice before adding broad
    functionality.
5.  Keep the application buildable and testable at every milestone.
6.  Do not introduce large frameworks or dependencies without explaining
    why they are needed, their license, platform coverage, maintenance
    status, and replacement strategy.
7.  Prefer small, replaceable dependencies behind project-owned
    interfaces. Do not expose GUI, audio-device, serialization, or
    third-party plug-in types through the core domain API.
8.  Do not use network services, accounts, telemetry, or cloud
    dependencies for core DAW functionality. The application must work
    offline.
9.  Do not claim real-time audio safety merely because the code
    compiles. Review allocations, locks, I/O, and blocking operations on
    the audio thread.
10. If this specification leaves a decision open, document the options
    and recommend a small, reversible decision rather than silently
    locking the project into a framework.

The goal is an extensible DAW that can grow over time---not a prototype
whose core is tightly coupled to a UI toolkit or audio backend.

## 2. Product vision

Build a modern, approachable, professional-minded DAW for recording,
arranging, editing, processing, mixing, and exporting music. The first
release should support a reliable multitrack workflow for audio and
MIDI, live instrument recording, non-destructive effects, basic
automation, and stereo export.

The product should feel like a coherent desktop music-production
environment, not a collection of disconnected tools. Its main workspace
should emphasize the arrangement timeline, with transport controls
always easy to reach and mixer/editor panels available without opening
many unrelated windows.

### 2.1 Product principles

-   **Open and free:** the complete application is free to use, and the
    project is developed under an explicitly chosen open-source license.
-   **Cross-platform:** Windows, macOS, and Linux are first-class
    targets.
-   **User ownership:** projects and recorded media are stored locally
    in documented, versioned formats. Users should not need an account
    or a vendor service to open their work.
-   **Extensibility:** instruments, effects, routing, project data, and
    UI panels should be designed for growth.
-   **Replaceable infrastructure:** OS audio APIs, audio-device
    libraries, GUI frameworks, and future plug-in SDK integrations must
    be replaceable behind narrow interfaces.
-   **Reliable recording:** recording must prioritize correctness, clear
    status, safe media writing, and recoverability.
-   **Non-destructive editing:** edits and effect changes should
    normally preserve original source media.
-   **Real-time discipline:** audio processing must be deterministic and
    bounded where it runs on the real-time thread.
-   **Usability:** common actions should be visible, discoverable,
    undoable, and accessible by keyboard.
-   **Testability:** the engine and project model must work in offline
    tests without requiring physical audio hardware.

### 2.2 Intended users

-   Musicians recording guitars, microphones, keyboards, and other
    instruments.
-   Producers arranging audio and MIDI parts.
-   Electronic musicians working with software instruments and effects.
-   Developers and advanced users who value transparent formats and
    extensibility.

### 2.3 Non-goals for the first MVP

Do not attempt these in the initial release:

-   Full third-party VST3, Audio Unit, or LV2 hosting.
-   A large commercial-grade sample library.
-   Advanced clip-launch/session performance mode.
-   Surround/immersive audio and sophisticated mastering workflows.
-   Collaborative cloud projects or online accounts.
-   Built-in distribution/marketplace for plug-ins.
-   Full notation engraving.
-   Mobile applications.
-   A networked or microservice architecture.

The architecture should leave room for future plug-in hosting and
additional workflows without requiring them in the first milestone.

## 3. Proposed technology direction

### 3.1 Core stack

-   **Rust:** domain model, project model, commands, DSP, engine, media
    management, offline renderer, application services.
-   **Cargo workspace:** split into crates when the boundaries are
    useful and clear.
-   **GUI framework:** not yet irrevocably selected. Evaluate a
    native-feeling Rust UI approach such as egui, and compare
    alternatives against docking/resizing, accessibility, high-DPI
    support, performance, native file dialogs, and cross-platform
    behavior. Keep the GUI behind an application-facing interface so it
    can be replaced.
-   **Audio I/O:** evaluate CPAL or thin native platform adapters. Treat
    the chosen library as a replaceable device-I/O layer, not as the
    audio engine itself.
-   **MIDI I/O:** introduce through a separate adapter and interface.
-   **Build and quality:** Cargo, rustfmt, Clippy, unit/integration
    tests, CI on Windows/macOS/Linux, and dependency/license auditing.

These are proposed starting points, not permission to add dependencies
without review. Check current compatibility and licenses before adopting
a crate.

### 3.2 Licensing

Choose the project license explicitly before publishing.
GPL-3.0-or-later is one option if the project should preserve copyleft
for derivative distributions; a permissive license is another option if
broad reuse is more important. Do not assume that all dependencies are
compatible with the chosen license.

Maintain a dependency inventory with: - crate and version; - purpose; -
license; - platforms supported; - reason for using it; - whether it can
be replaced; - relevant upstream links.

Do not copy source code from other DAWs without checking its license and
obligations. Learning from public behavior or general architectural
patterns is not the same as copying implementation code.

## 4. High-level architecture

Use a **modular monolith**, not microservices. The application is one
desktop program with well-separated internal modules.

Conceptual flow:

``` text
UI / User Input
      |
      v
Application Commands + UI State
      |
      v
Project Model <----> Undo / Redo / Serialization
      |
      v
Engine Update / Render Graph
      |
      +------------------------+
      |                        |
      v                        v
Real-time Audio Engine    Offline Render Engine
      |                        |
      v                        v
Audio Device Adapter      WAV / Render Output
```

The UI must not directly mutate memory that the audio thread is actively
reading. UI actions should become commands or parameter updates,
validated and applied through a safe handoff mechanism.

### 4.1 Architectural layers

1.  **Domain/core:** stable IDs, tracks, clips, notes, parameters,
    routing, automation, time representation, validation rules.
2.  **Project/session:** project lifecycle, asset references,
    serialization, version migration, autosave, recovery.
3.  **Application services:** commands, undo/redo, selection,
    import/export orchestration, device configuration workflows.
4.  **Audio engine:** transport scheduling, audio graph, mixing,
    processing, monitoring, latency handling.
5.  **DSP and instruments:** reusable processors and sound generators
    that do not depend on the GUI.
6.  **Platform adapters:** audio/MIDI devices, native file dialogs,
    OS-specific integration.
7.  **UI:** arrangement, mixer, editors, browser, inspectors,
    preferences and dialogs.

### 4.2 Proposed Cargo workspace

Start with a small number of crates and split as real boundaries emerge.
A possible future layout:

``` text
free-sound/
├── Cargo.toml
├── crates/
│   ├── daw-core/          # IDs, time, domain types, validation
│   ├── daw-project/       # project files, media references, migrations
│   ├── daw-commands/      # commands, undo/redo
│   ├── daw-dsp/           # built-in DSP processors
│   ├── daw-engine/        # graph, transport, mixing, scheduling
│   ├── daw-audio-io/      # audio-device abstraction and adapters
│   ├── daw-midi/          # MIDI model, scheduling and device adapter
│   ├── daw-render/        # offline rendering and export
│   └── daw-app/           # GUI and application composition
├── docs/
│   ├── architecture/
│   ├── adr/
│   └── testing/
└── tests/
```

Do not create empty crates for every future feature at the beginning.
The initial implementation can begin with `daw-core`, `daw-project`,
`daw-dsp`, `daw-engine`, and `daw-app`, then split when interfaces
stabilize.

### 4.3 Interface ownership

-   Core types must not contain GUI-framework types.
-   Core types must not contain audio-device-library types.
-   The project format must not serialize raw Rust memory layouts or
    unstable internal type representations.
-   DSP processors must not know which UI widget controls them.
-   A processor must not directly write project files or perform disk
    I/O.
-   UI components must not own audio callback buffers.
-   Platform-specific code must be localized in adapters.

## 5. Core project concepts and data model

Use stable IDs instead of relying on array positions or display names.
Names can change; references must remain valid.

### 5.1 Project

A project contains: - project ID and display name; - project-format
version; - sample rate and default tempo/time signature; - ordered
tracks and buses; - arrangement markers and loop range; - media-asset
references; - automation data; - routing graph; - UI-independent
musical/project state; - optional project metadata.

UI preferences such as panel sizes, selected tab, and last zoom should
be stored separately from essential musical content where practical.

### 5.2 Tracks

Initial track types: - **Audio track:** plays audio clips and may record
from an input. - **MIDI/instrument track:** stores MIDI clips and sends
events to a built-in instrument. - **Bus track:** sums routed signals
and may host effects. - **Master track:** final output processing and
metering.

Each track has a stable ID, name, color, order, mute/solo state, volume,
pan where applicable, routing, insert-effect chain, and automation
targets.

Record-arm and monitoring state are operational state, not necessarily
persistent project content. On project reopen, do not silently begin
recording or open a live input monitor.

### 5.3 Clips and source material

-   **Audio clip:** references an audio asset and contains a timeline
    start, source offset, duration, gain, fades, and edit metadata.
-   **MIDI clip:** contains note and controller events with a timeline
    range and musical position.
-   Clips should be movable, trim-able, split-able, duplicated, and
    deleted non-destructively.
-   The project must distinguish clip position from source-media offset.
-   Moving a clip must not rewrite the source audio file.
-   Missing media should be detected and reported with a relink
    workflow.

### 5.4 Time representation

Audio processing uses sample frames and sample rate. Musical editing
uses beats/bars/ticks or another well-defined beat-time representation.
The conversion between musical time and sample time must account for
tempo changes if tempo automation is added.

Do not use floating-point seconds as the only authoritative
representation for clip and note positions. Avoid cumulative rounding
errors. Define the timing model and rounding behavior in tests.

### 5.5 Automation

An automation lane targets a stable parameter ID and stores time/value
points plus an interpolation mode. It must be possible to automate track
volume and selected effect parameters in the MVP.

Initial interpolation options: - step/hold; - linear.

Support additional curves later if needed. Validate parameter ranges and
define how automation behaves when clips or regions are moved.

### 5.6 Effects and parameters

An effect instance has: - stable instance ID; - processor type
identifier; - enabled/bypassed state; - ordered parameter values; -
optional preset metadata; - reported latency where relevant; -
channel-layout requirements.

Parameters should have stable IDs, display names, units, valid ranges,
default values, and a mapping between normalized UI values and actual
DSP values. Automation must refer to parameter IDs, not to widget
positions.

## 6. Project files and media management

The project must be portable, documented, versioned, and recoverable.

### 6.1 Proposed project layout

Evaluate a directory-based project format:

``` text
MySong.dawproject/
├── project.json-or-binary
├── media/
│   ├── audio/
│   └── imported/
├── peaks/
├── presets/
├── autosave/
└── recovery/
```

The exact serialization format is an ADR decision. Use a schema with
explicit versioning and migrations; do not simply serialize arbitrary
internal Rust structs and assume they will remain compatible.

### 6.2 Requirements

-   Save and load project content reliably.
-   Autosave at a configurable interval and after meaningful operations
    where appropriate.
-   On crash/restart, offer recovery without silently overwriting the
    last valid project.
-   Support collecting/copying referenced media into a project folder.
-   Detect missing, inaccessible, or changed media.
-   Avoid duplicate imports where a safe content identity can be
    established.
-   Keep waveform peak files rebuildable; they are caches, not
    authoritative audio.
-   Use atomic or staged saves so an interrupted save does not corrupt
    the last good project.
-   Clearly distinguish "Save," "Save As," "Collect/Copy Media," and
    "Export Audio."

## 7. UI and interaction specification

### 7.1 Overall visual direction

Use a clean, modern, dark-first interface with restrained color, strong
contrast, clear typography, and dense but readable controls. Avoid
copying another DAW's exact visual identity. A light theme can be added
after the dark theme is usable.

Proposed visual tokens (treat as initial values, not immutable
branding): - app background: `#20242C`; - panel background: `#2B303A`; -
primary accent: `#73C7B8`; - recording/danger: `#F16C73`; - text:
high-contrast neutral; - secondary text: muted neutral.

Use spacing and sizing tokens rather than scattered hard-coded values.
Support high-DPI scaling, resizing, keyboard focus, and legible controls
at different window sizes.

### 7.2 Main window layout

Use a single main window with five primary regions:

1.  **Menu/project bar:** top; project name, File, Edit, View, Track,
    Create, Help, and window-level actions.
2.  **Transport bar:** directly below the menu; play, stop, record,
    loop, metronome/count-in, tempo, time signature, playhead position,
    and audio status.
3.  **Arrangement workspace:** center and largest region; track headers
    on the left and timeline/clip area on the right.
4.  **Mixer panel:** docked bottom or toggled panel; channel strips,
    meters, faders, routing, master.
5.  **Browser/inspector:** docked left or right; instruments, effects,
    samples, presets, track settings, and contextual properties.

Panels should be resizable, collapsible, and restore their last layout.
The arrangement must remain usable on smaller screens. Avoid requiring
many floating windows for routine work.

### 7.3 Main toolbar and transport

Include: - play, stop, record; - return-to-start; - loop enable and
loop-range controls; - metronome; - count-in; - tempo; - time
signature; - current position shown in bars/beats and/or time; -
audio-device/status indicator; - sample rate and buffer information in a
status or settings panel; - CPU/load and dropout indication when
implemented.

Use a clear visual state for stopped, playing, recording, and error
conditions. The record state must not be conveyed by color alone. A
device failure or audio dropout should be discoverable without opening
logs.

### 7.4 Arrangement screen

**Track header area** - track number and editable name; - track-type
icon; - track color; - record-arm button for recordable tracks; -
input-monitor toggle; - mute and solo; - track level/meter preview if
space allows; - collapse/expand for lanes or track details.

**Timeline area** - bar/beat ruler and time ruler where useful; -
vertical grid lines and configurable snap; - playhead and loop range; -
audio waveform clips; - MIDI note-preview clips; - automation lanes; -
markers; - horizontal and vertical scrolling; - zoom controls and
fit-to-project; - drag/drop clip operations.

**Clip interactions** - click to select; - drag to move; - edge drag to
trim; - split at playhead or selected location; - duplicate; - delete; -
drag selected clips with snapping; - optional modifier for fine movement
or duplicate; - contextual menu for common actions; - undo/redo for
destructive-looking editing operations.

Do not make a clip's source file change when the clip is moved, trimmed,
or split. Splitting should create clip references/regions over the same
underlying source where possible.

### 7.5 Track inspector

When a track is selected, show contextual properties: - name and
color; - track type; - audio input selection for audio tracks; -
monitoring and record-arm state; - instrument selection for MIDI
tracks; - volume and pan; - output routing; - insert-effect chain; -
sends; - automation targets; - track-specific options.

The inspector should not duplicate every mixer control unnecessarily.
Changes made in the inspector and mixer must update the same underlying
project state.

### 7.6 Mixer screen

Each channel strip should contain: - track/bus name and color; -
input/output or routing summary; - level meter with peak hold; - fader
and numeric dB value; - pan control; - mute and solo; - access to insert
effects; - send controls; - clipping indicator with a clear reset
action.

Master channel: - output device/bus selection; - stereo level meter; -
clipping indicator; - master volume; - optional master insert chain.

MVP routing should support track-to-master output and simple buses.
Design the graph to support sends and more flexible routing later, but
do not make the first version's routing UI overly complex.

### 7.7 MIDI editor / piano roll

Layout: - toolbar at top with snap, quantize, note-length tools, and
draw/select modes; - piano keyboard vertically on the left; - beat grid
horizontally; - MIDI notes displayed as rectangles; - velocity lane at
bottom; - optional controller lanes for supported MIDI CC values; -
note/event inspector for precise values.

Interactions: - draw a note; - select, move, resize, duplicate, and
delete notes; - change pitch and velocity; - quantize selected notes or
a clip; - zoom and scroll; - audition notes when appropriate; -
undo/redo.

The MIDI editor must work with a built-in instrument and must not assume
that all MIDI tracks are piano-like. It should remain usable for drums
and other note-based instruments.

### 7.8 Effect editor and chain

An effect chain shows ordered processors as rows/cards with: - effect
name and icon; - enable/bypass control; - open-editor control; - drag
handle for reordering; - remove action; - add-effect action.

An effect editor has a common header: - effect name; - enabled/bypassed
state; - preset selector/save action when supported; - reset/default
action; - parameter controls; - automation/parameter-link access.

Use specialized controls where they communicate the DSP well: a
frequency curve for EQ, time/feedback controls for delay, gain-reduction
display for compressor if feasible, and drive/tone controls for
distortion. Do not require elaborate visualizations for the first
release.

### 7.9 Browser

Provide categories: - Instruments; - Effects; - Samples/audio files; -
Presets; - Recent items; - optionally project media.

Include search and filtering. Support drag-and-drop of an
asset/effect/instrument to a valid target. Show a useful message when an
asset is unsupported or cannot be loaded. Avoid network-dependent
discovery.

### 7.10 Recording/device preferences

Device configuration screen should show: - available audio
host/device; - input and output devices; - channel mappings; - sample
rate; - buffer size; - input/output latency values if available; - test
input/output meters; - MIDI input devices; - current device status and
errors.

The interface should explain that smaller buffers can reduce monitoring
latency but may increase CPU pressure and dropouts. Do not promise a
latency value the backend cannot measure or estimate reliably.

### 7.11 Dialogs and feedback

Required dialogs/workflows: - new project; - open/save/save as; - import
audio; - export audio; - missing media/relink; - device
selection/error; - unsaved changes; - recovery after abnormal
shutdown; - unsupported file/format; - overwrite confirmation.

Dialogs should be concise and actionable. Error messages should explain
what happened, what remains safe, and the next step. Never silently
discard a recording or project change.

### 7.12 Accessibility and usability

-   keyboard navigation and visible focus;
-   tooltips for unfamiliar controls;
-   labels or accessible names for icon-only buttons;
-   no reliance on color alone for armed/recording/clipping states;
-   scalable text and controls;
-   configurable shortcuts where practical;
-   context menus as an additional path, not the only path;
-   confirmation for irreversible file deletion or destructive media
    operations.

## 8. Main user workflows

### 8.1 Create a project

1.  User chooses New Project.
2.  App requests project name and location, and optionally a sample
    rate.
3.  App creates the project structure and initial master track.
4.  Arrangement opens with an empty timeline.
5.  App indicates whether a usable audio device is configured.
6.  User can work on the project even if no physical device is present;
    offline editing and rendering must remain possible.

### 8.2 Record a live guitar or microphone

1.  Create or select an audio track.
2.  Select an input device/channel.
3.  Enable input monitoring if desired.
4.  Check input level and avoid clipping.
5.  Arm the track.
6.  Optionally enable metronome/count-in and set a loop or punch range.
7.  Press Record.
8.  The engine captures audio into a safe recording pipeline while
    playback continues.
9.  The UI displays recording state, elapsed time, input level, and
    disk/recording warnings where available.
10. Stop recording.
11. Finalize the media file and create an audio clip in the arrangement.
12. Generate waveform peaks asynchronously.
13. Save project state and ensure the recorded asset is referenced
    correctly.

The audio callback must not write arbitrary file operations
synchronously. Use a bounded buffer/ring-buffer or equivalent handoff to
a dedicated writer thread. Define behavior for buffer overflow,
disk-full conditions, device loss, and interrupted recording. Never
silently pretend a failed recording succeeded.

### 8.3 Record MIDI and use a software instrument

1.  Create a MIDI/instrument track.
2.  Choose a built-in instrument.
3.  Select a MIDI input device if recording from a keyboard or
    controller.
4.  Arm the track and optionally enable monitoring.
5.  Record note and controller events synchronized to the transport.
6.  Create a MIDI clip.
7.  Open the piano roll to correct timing, pitch, length, and velocity.
8.  Adjust the instrument and effects without altering the recorded
    MIDI.
9.  Play back through the audio engine.
10. Optionally bounce/freeze the instrument output to audio in a later
    milestone.

MIDI events must be scheduled against the audio timeline. Do not rely on
UI frame timing for musical accuracy.

### 8.4 Edit audio

-   move and trim clips;
-   split clips at the playhead;
-   duplicate/delete clips;
-   adjust clip gain;
-   apply fades and later crossfades;
-   zoom waveform and timeline;
-   undo/redo;
-   retain the original source file.

Edits should be represented as project metadata whenever possible. If a
future operation must destructively alter media, it must be explicit and
reversible where feasible.

### 8.5 Mix multiple tracks

1.  Add audio and/or MIDI tracks.
2.  Route each track to the master or a bus.
3.  Adjust faders and pan.
4.  Add insert effects in the desired order.
5.  Use mute/solo and meters to diagnose the mix.
6.  Add simple sends/buses when implemented.
7.  Detect clipping and indicate it clearly.
8.  Export the final stereo mix.

The mixer should be usable while playback is running. Parameter changes
must reach the audio engine safely and without blocking the callback.

### 8.6 Add distortion and automate a wah effect

Example chain:

``` text
MIDI clip -> Instrument -> Distortion -> Wah filter -> Reverb -> Track mixer
```

For an audio guitar recording, omit the instrument stage:

``` text
Audio clip -> Distortion -> Wah filter -> Reverb -> Track mixer
```

The original MIDI or audio remains intact. The user can: - change
distortion drive after recording; - bypass or reorder effects; - draw a
wah-frequency automation curve; - record a parameter movement where
supported; - edit automation points later; - render the result to audio.

Automation must be a general feature. Do not implement a special "wah
automation" subsystem.

### 8.7 Save, reopen, and recover

-   Save stores project state and media references safely.
-   Autosave creates recoverable state without corrupting the primary
    project.
-   On reopening, missing media is reported and can be relinked.
-   Device settings are restored where possible, but the app must
    gracefully handle unavailable devices.
-   Recovery should show the user the available recovery point and avoid
    silently replacing the primary project.

### 8.8 Export a mix

Initial export requirements: - stereo WAV; - sample-rate selection from
supported values; - bit-depth selection from supported formats; -
full-project or selected-range export; - clear output path and
filename; - progress indicator and cancel action if safe; - error
reporting for disk/full/format failures.

Offline export should not require a physical audio device. The offline
renderer should run as fast as practical while preserving the intended
sound and timing. Document any differences between real-time and offline
processing.

## 9. Audio engine and real-time processing

This is the most technically sensitive part of the project.

### 9.1 Audio engine responsibilities

-   maintain the playback transport;
-   schedule clips and MIDI events against sample time;
-   read/decode media without blocking the real-time callback;
-   render instruments;
-   process audio through a graph of processors;
-   mix tracks and buses;
-   handle volume, pan, mute, solo, and routing;
-   deliver output buffers to the device adapter;
-   capture input for recording;
-   report meters, clipping, CPU/load and dropout information;
-   support offline rendering using the same DSP logic where practical.

### 9.2 Real-time callback rules

The real-time audio callback must avoid: - heap allocations and
deallocations where avoidable; - blocking mutexes/RW locks; - filesystem
or network I/O; - waiting for another thread; - unbounded loops or
unpredictable work; - logging that may block; - expensive UI
operations; - constructing/destroying complex processor graphs.

Preallocate working buffers, prepare processors before playback, and use
bounded communication structures for control and event transfer. Define
overload behavior explicitly. If an operation cannot safely be completed
in real time, schedule it for a safe point or perform it on another
thread.

The audio thread should process fixed-size or backend-provided blocks
without assuming every block is exactly the same size unless the backend
guarantees it.

### 9.3 Graph and routing model

Represent signal flow as a directed graph with explicit node and
connection identities. Initial node types: - audio clip source; -
instrument source; - insert processor; - track mixer/channel; - bus; -
master output.

Prevent invalid cycles in the initial routing model unless a
well-defined feedback-routing design is intentionally added later.
Validate channel layouts and routing before playback. Build or update
the graph outside the real-time callback, then switch to a valid
prepared graph at a safe boundary.

Keep graph topology separate from parameter values. Parameter updates
should not require rebuilding the whole graph.

### 9.4 Audio buffers and channels

-   Use a clear internal sample format, likely `f32`, with conversion in
    adapters as required.
-   Represent mono/stereo and future multichannel layouts explicitly.
-   Do not assume every input, effect, or output is stereo.
-   Avoid unnecessary copies; where copies are required, make them
    deliberate and testable.
-   Handle channel mismatches predictably.
-   Provide a defined clipping/limiting policy; do not silently clip
    intermediate signals without documenting it.

### 9.5 Transport and scheduling

Transport state includes: - stopped/playing/recording; - sample
position; - musical position; - tempo and time signature; - loop
range; - count-in and metronome state; - punch-in/out range if
implemented.

Use a single authoritative clock/timeline model. MIDI event scheduling
must be sample-accurate or as accurate as the chosen engine architecture
permits. UI animation and redraw frequency must never determine note
timing.

### 9.6 Latency

Treat these as separate concepts: - **Monitoring latency:** delay
between live input and the musician hearing the processed signal. -
**Recording alignment/latency compensation:** alignment of recorded
material to its intended timeline position. - **Processor latency:**
delay introduced by a processor. - **Device/backend latency:** delay
reported or estimated by the audio subsystem.

Expose available settings and measurements honestly. Do not claim exact
latency compensation until the engine and backend can support it. Design
the graph to report processor latency so compensation can be added
later.

### 9.7 Recording pipeline

Use a bounded handoff from capture callback to a dedicated media-writer
thread. The writer finalizes files and reports errors. Requirements: -
no blocking disk writes in the callback; - explicit buffer-overrun
detection; - explicit disk-write failure handling; - recoverable partial
recording metadata; - asynchronous waveform/peak generation; -
device-disconnect handling; - safe stop/finalize sequence; - tests that
simulate slow or failed storage where possible.

### 9.8 Metering and diagnostics

Provide peak meters for tracks and master. The engine should be able to
report: - peak level; - clipping events; - audio callback load or
duration where measurable; - xruns/dropouts where the backend exposes
them; - current device/sample-rate/buffer status.

Transfer meter snapshots to the UI through a non-blocking, bounded or
otherwise real-time-safe mechanism. Do not update GUI widgets from the
audio thread.

## 10. DSP and built-in instruments

DSP processors should be small, composable, testable units. They should
process buffers and events, not know about tracks, project files,
windows, or UI controls.

### 10.1 Proposed processor contract

Use this only as a conceptual starting point; refine it based on actual
engine requirements:

``` rust
pub trait AudioProcessor: Send {
    fn prepare(&mut self, config: &ProcessConfig);
    fn process(
        &mut self,
        input: &[&[f32]],
        output: &mut [&mut [f32]],
        events: &[MidiEvent],
    );
    fn reset(&mut self);
}
```

Before finalizing the API, resolve: - in-place versus separate
input/output processing; - channel-layout negotiation; - processor
latency reporting; - parameter smoothing; - MIDI/event delivery; -
processor state save/restore; - bypass behavior; - denormal handling if
relevant; - real-time thread ownership and destruction; - offline-render
behavior.

Do not force this exact interface if it proves unsuitable. Record the
final contract in an ADR and test it.

### 10.2 Initial built-in processors

Implement in small increments:

1.  **Gain:** linear gain with safe parameter updates.
2.  **Pan:** documented pan law and mono/stereo behavior.
3.  **Mixer/summing:** combine routed signals without accidental
    clipping assumptions.
4.  **Distortion/drive:** nonlinear shaping with drive and output
    controls.
5.  **Filter/wah:** resonant filter with controllable cutoff/frequency;
    smooth parameter changes to avoid zipper noise.
6.  **Basic EQ:** begin with a small number of useful bands or filters.
7.  **Delay:** time, feedback, wet/dry; guard against unstable feedback
    values.
8.  **Reverb:** start with a simple algorithmic reverb and parameters
    appropriate to the implementation.
9.  **Compressor:** threshold, ratio, attack, release, makeup gain;
    expose reduction metering when practical.

Use deterministic tests with known input signals and tolerances. Add
performance benchmarks for processors that run per audio block.

### 10.3 Instrument

MVP should include one basic built-in instrument. A simple synthesizer
can include: - oscillator/waveform selection; - amplitude envelope; -
filter with cutoff/resonance; - MIDI note-on/note-off; - velocity
response; - pitch bend or a clearly defined subset of controllers if
practical; - preset state save/restore.

Do not attempt a comprehensive commercial synth at first. The purpose is
to validate MIDI scheduling, voice management, instrument state, and
rendering.

### 10.4 Parameter smoothing

Parameters such as gain, cutoff, pan, and wet/dry should not change
abruptly if abrupt changes create clicks or zipper noise. Implement
parameter smoothing or sample/block interpolation where appropriate.
Define which parameters can be updated per block and which require more
specialized handling.

### 10.5 Processor state and presets

-   Each processor must serialize its state through a versioned
    representation.
-   State must survive project save/reopen.
-   Unknown or removed processor IDs should produce a recoverable
    missing-effect state rather than corrupting the project.
-   Presets should use documented/versioned data.
-   Resetting a parameter should be undoable where practical.

## 11. MIDI system

MVP MIDI support includes: - MIDI clip and note data; - note-on/note-off
and velocity; - external MIDI input where supported by the adapter; -
recording synchronized to transport; - piano-roll editing; -
quantization; - MIDI event scheduling; - a built-in instrument.

Design for future controller data and automation, but do not claim
support for every MIDI message until it is implemented and tested.
Handle stuck notes safely on stop, transport jumps, device disconnect,
and project shutdown.

Keep MIDI event timing independent from UI refresh timing. Define
behavior for events at loop boundaries and for notes spanning a loop
point.

## 12. Audio media and file handling

-   Import common uncompressed audio formats first; select decoding
    dependencies only after license/platform review.
-   Keep imported source media immutable by default.
-   Store sample rate, channel count, duration/frame count, and other
    necessary metadata.
-   Resample when source rate differs from the engine/export rate, using
    a well-tested implementation.
-   Generate waveform peaks in background jobs.
-   Do not load entire long recordings into RAM unnecessarily.
-   Detect unsupported, corrupt, truncated, or inaccessible files.
-   Use temporary files and safe finalization for recorded media.
-   Never allow waveform-cache failure to invalidate the source audio.

## 13. Undo/redo and command model

Most user-visible editing operations should go through commands: -
create/delete/rename track; - move/trim/split/duplicate clip; -
add/remove/reorder effect; - set parameter; - edit automation; - change
routing; - create/move/delete MIDI note; - edit project settings.

A command should validate its operation, apply the change, and provide
enough information to reverse it. Define whether a command is safe
during playback and how engine updates are scheduled.

Do not place every high-frequency parameter sample into the normal undo
history. Parameter gestures should be grouped into one undoable action,
such as one fader drag or one automation-drawing gesture.

Recording itself requires special treatment: the recorded media must be
finalized safely, and undoing a recorded clip should not accidentally
destroy the only copy of its source media.

## 14. Threading and communication

Potential worker responsibilities: - real-time audio callback; -
audio/media writer; - waveform peak generation; - import
decoding/resampling; - offline rendering; - UI thread.

Use clear ownership and bounded queues where real-time communication is
required. Decide what happens when queues are full. Never let background
workers mutate UI state directly without a controlled message/event
path.

The application must remain responsive while: - playing audio; -
recording; - generating waveforms; - importing media; -
rendering/exporting; - scanning the browser.

Long operations should provide progress and cancellation when
cancellation is safe.

## 15. MVP feature scope and priorities

### P0 --- Foundation and first vertical slice

-   Rust workspace and cross-platform CI.
-   Project model with stable IDs.
-   Versioned save/load.
-   Basic main window and transport.
-   Offline audio buffer processing.
-   Gain processor and simple mixer.
-   Unit tests for timing, project serialization, and DSP.
-   Architecture notes and dependency/license inventory.

**Exit criteria:** application builds on target platforms; an empty
project can be created, saved, reopened, and tested without audio
hardware.

### P1 --- Playback and arrangement

-   Audio clip model and import.
-   Timeline ruler, tracks, playhead, clip selection/movement.
-   Audio playback through a device adapter.
-   Track and master gain.
-   Basic meters.
-   WAV offline export.
-   Undo/redo for core arrangement actions.

**Exit criteria:** user can import an audio file, place it on a
timeline, play it, adjust level, save/reopen, and export a stereo mix.

### P2 --- Reliable recording

-   Device/input selection.
-   Track arm and monitoring.
-   Recording pipeline with writer thread.
-   Waveform generation.
-   Recording status and errors.
-   Autosave/recovery and missing-media handling.
-   Count-in/metronome if feasible.
-   Initial latency diagnostics.

**Exit criteria:** user can record a live input while playback runs,
stop safely, reopen the project, and hear the recorded material aligned
reasonably with the timeline. Failure cases must be tested.

### P3 --- Mixer and effects

-   Mixer channel strips.
-   Insert chain and effect ordering.
-   Gain, pan, distortion, filter, delay, reverb.
-   Basic EQ and compressor as schedule allows.
-   Effect state persistence and bypass.
-   Track/bus routing at a basic level.
-   Automation for track volume and one or more effect parameters.

**Exit criteria:** user can change effect settings after recording
without modifying source media, automate a parameter, save/reopen, and
export the processed sound.

### P4 --- MIDI and instrument

-   MIDI clips and event model.
-   MIDI input adapter.
-   MIDI recording and scheduling.
-   Piano roll and velocity editing.
-   Built-in synthesizer.
-   Quantization.
-   MIDI-to-audio offline render path.

**Exit criteria:** user can record/edit MIDI, hear it through the
built-in instrument in sync with audio, and export a mixed result.

### P5 --- Hardening and usability

-   Project recovery polish.
-   Device loss/dropout reporting.
-   Performance benchmarks and profiling.
-   Keyboard shortcuts and accessibility pass.
-   Responsive panel layout.
-   More robust import/export options.
-   Installer/package CI for all target platforms.
-   Documentation and sample project.

Third-party plugin hosting, advanced comping, sophisticated crossfades,
clip-launch mode, and advanced routing remain later milestones.

## 16. Testing strategy

### 16.1 Unit tests

-   time conversion and tempo mapping;
-   clip boundaries and split/trim math;
-   serialization round-trip and migrations;
-   automation interpolation;
-   gain/pan/filter/distortion behavior;
-   MIDI note/event scheduling;
-   routing validation and cycle rejection;
-   undo/redo inversion;
-   parameter-range validation.

### 16.2 Integration tests

-   create/save/reopen a project;
-   import media and resolve references;
-   play a known test signal through a graph;
-   offline-render a project and inspect output frame count/levels;
-   record through a simulated capture source;
-   recover from interrupted recording metadata;
-   load projects containing missing/unknown effects;
-   verify automation and effect state survive save/reopen.

### 16.3 Real-time and performance tests

-   benchmark processors at representative sample rates and block sizes;
-   measure graph-processing time;
-   stress-test many tracks and effects;
-   simulate a slow disk writer and full bounded queue;
-   verify no blocking filesystem operations occur on the callback path;
-   test device removal/reconnection where feasible.

Avoid flaky tests that depend on exact wall-clock timing or a specific
physical sound card. Use deterministic fake devices and offline buffers
for most CI tests.

### 16.4 Platform acceptance

Run builds/tests on Windows, macOS, and Linux. Track platform-specific
limitations explicitly. Verify audio device enumeration, input/output
configuration, file dialogs, keyboard behavior, high-DPI layout, and
packaging on each OS.

## 17. Definition of done

A feature is not complete merely because it compiles. It should include,
as applicable: - documented behavior; - tests; - error handling; -
undo/redo behavior; - persistence behavior; - keyboard/accessibility
behavior; - real-time safety review if it touches the audio engine; -
platform impact review; - dependency/license review if a dependency is
added; - user-facing status/error feedback; - a clear acceptance test.

## 18. Implementation workflow

Work in short milestones. For each milestone, provide: 1. **Goal and
scope.** 2. **Files/modules to add or modify.** 3. **Architecture
decisions and rationale.** 4. **Acceptance criteria.** 5. **Tests.** 6.
**Known limitations.** 7. **Next recommended milestone.**

Before implementation, create: - `docs/architecture/overview.md`; -
`docs/architecture/audio-engine.md`; -
`docs/architecture/project-format.md`; - `docs/architecture/ui.md`; -
`docs/adr/` for decisions; - `docs/testing/strategy.md`.

Do not generate a giant codebase in one pass. First validate the project
model, command flow, and offline audio processing with a small vertical
slice. Then add device I/O and recording. Build the UI around real
domain concepts rather than speculative placeholder structures.

## 19. Initial decisions that must be recorded as ADRs

1.  Project license and contribution policy.
2.  GUI framework and docking/panel strategy.
3.  Audio I/O library and platform-specific fallback strategy.
4.  Internal audio sample format and channel-layout model.
5.  Project directory and serialization format.
6.  Time representation for musical and sample positions.
7.  Real-time command/parameter communication design.
8.  Processor API and graph update strategy.
9.  Media decoding/resampling libraries and licenses.
10. MIDI input/output library.
11. Installer and packaging strategy.

Do not make all these decisions prematurely. Prioritize choices that
affect foundational interfaces; keep others open until a concrete
feature needs them.

## 20. First concrete task

Start by inspecting the repository and then implement **Milestone 0:
architecture skeleton and offline vertical slice**.

Deliver: - a concise repository assessment; - proposed Cargo workspace
and minimal crate boundaries; - ADRs for the initial high-impact
choices, clearly marking unresolved decisions; - a minimal
project/session model with stable IDs; - versioned save/load
round-trip; - an offline audio buffer abstraction; - a gain processor
and simple summing/mixing function; - deterministic unit tests; - a
minimal UI shell that opens and displays an empty project, without
pretending recording or playback is already implemented; - build/test
instructions for Windows, macOS, and Linux.

Do not implement live recording, a full arrangement editor, plug-in
hosting, or a complex DSP library in Milestone 0. Confirm the
architecture works, then proceed to the next milestone.
