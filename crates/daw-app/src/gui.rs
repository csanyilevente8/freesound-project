//! egui-based graphical prototype (ADR-0002).
//!
//! Feature-gated behind `gui` so the default/headless build never pulls
//! windowing/GPU dependencies. This is a **prototype** to validate egui against
//! the ADR-0002 gates — a custom timeline, a transport bar driving live
//! playback, keyboard control, and accessible labels — not a full DAW UI.
//!
//! It is a thin presenter over the existing layers: it holds a `Project`
//! (loaded via `daw-project`), draws it, and drives playback through
//! `crate::playback` + the `daw-audio-io` handle. No GUI type touches the core
//! crates.

use eframe::egui;
use eframe::egui::containers::{CentralPanel, Panel};

use daw_commands::commands::{
    AddClip, MoveClip, RemoveClip, SetClipGeometry, SetEffectBypass, SetTrackMute, SetTrackSolo,
    SetTrackVolume,
};
use daw_commands::History;
use daw_core::audio::Gain;
use daw_core::id::{ClipId, EffectId, TrackId};
use daw_core::project::Project;
use daw_core::time::SampleTime;
use daw_core::track::TrackKind;

use crate::playback::{self, LivePlayback};

/// Visual tokens from the spec §7.1 (initial dark-first values).
mod theme {
    use eframe::egui::Color32;
    pub const APP_BG: Color32 = Color32::from_rgb(0x20, 0x24, 0x2C);
    pub const PANEL_BG: Color32 = Color32::from_rgb(0x2B, 0x30, 0x3A);
    pub const ACCENT: Color32 = Color32::from_rgb(0x73, 0xC7, 0xB8);
    pub const DANGER: Color32 = Color32::from_rgb(0xF1, 0x6C, 0x73);
    pub const CLIP: Color32 = Color32::from_rgb(0x3E, 0x5A, 0x6E);
}

/// The GUI application state.
pub struct DawGui {
    project: Project,
    project_path: Option<String>,
    /// Live playback session, created on first Play and kept alive so pause/
    /// resume retains position. `None` until first Play or after reset.
    playback: Option<LivePlayback>,
    /// Whether the transport is currently running (distinct from "a session
    /// exists" — a paused session exists but is not playing).
    playing: bool,
    /// Last-known playhead frame (from meter events) and peak levels.
    playhead: u64,
    peak_l: f32,
    peak_r: f32,
    status: String,
    /// Pixels per second for the timeline zoom.
    px_per_sec: f32,
    /// Undo/redo history for edits made in the GUI.
    history: History,
    /// Clip currently being dragged.
    drag: Option<ClipDrag>,
    /// Currently selected clip (for delete/split), if any.
    selected: Option<(TrackId, ClipId)>,
    /// Live master output gain (linear). Applied in the player in real time via
    /// `SetMasterGain` — changes are heard immediately without re-rendering.
    master_gain: f32,
}

/// What a clip drag is doing.
#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    /// Move the whole clip (change timeline_start, keep source_offset/duration).
    Move,
    /// Trim the left edge (change start + source_offset + duration together).
    TrimStart,
    /// Trim the right edge (change duration only).
    TrimEnd,
}

/// In-progress clip drag.
struct ClipDrag {
    track: TrackId,
    clip: ClipId,
    mode: DragMode,
    /// Original geometry at grab time.
    original_start: SampleTime,
    original_offset: SampleTime,
    original_duration: SampleTime,
    /// Pointer x when the drag began (screen px).
    grab_x: f32,
    /// Live preview geometry during the drag (committed on release).
    preview: Option<ClipGeom>,
}

/// A clip geometry snapshot (preview / commit).
#[derive(Clone, Copy, PartialEq)]
struct ClipGeom {
    start: SampleTime,
    offset: SampleTime,
    duration: SampleTime,
}

impl DawGui {
    /// Creates the GUI with an initial project (and optional source path).
    #[must_use]
    pub fn new(project: Project, project_path: Option<String>) -> Self {
        Self {
            project,
            project_path,
            playback: None,
            playing: false,
            playhead: 0,
            peak_l: 0.0,
            peak_r: 0.0,
            status: "ready".into(),
            px_per_sec: 120.0,
            history: History::new(),
            drag: None,
            selected: None,
            master_gain: 1.0,
        }
    }

    fn is_playing(&self) -> bool {
        self.playing
    }

    /// Ensures a playback session exists (rendering once), then starts/resumes
    /// the transport from the current position.
    fn resume_or_start(&mut self) {
        if self.playback.is_none() {
            let base = self
                .project_path
                .as_deref()
                .and_then(|p| std::path::Path::new(p).parent())
                .map(std::path::Path::to_path_buf);
            match playback::start(&self.project, base.as_deref()) {
                Ok(mut live) => {
                    // Apply the current master gain to the fresh session.
                    live.handle.set_master_gain(self.master_gain);
                    self.status =
                        format!("playing ({:.2}s @ {} Hz)", live.seconds(), live.device_rate);
                    self.playback = Some(live);
                }
                Err(e) => {
                    self.status = format!("play failed: {e}");
                    return;
                }
            }
        }
        // Resume: the player retains its exact position across a Stop/pause, so
        // just start the transport again (no seek — avoids snapping to the
        // slightly-stale meter playhead).
        if let Some(live) = &mut self.playback {
            live.handle.play();
            self.playing = true;
            self.status = "playing".into();
        }
    }

    /// Pauses the transport but keeps the session (position retained).
    fn pause(&mut self) {
        if let Some(live) = &mut self.playback {
            let _ = live.handle.stop();
        }
        self.playing = false;
        self.peak_l = 0.0;
        self.peak_r = 0.0;
        self.status = "paused".into();
    }

    /// Stops and returns to the start, tearing down the session.
    fn reset_to_start(&mut self) {
        self.playback = None; // drop the stream
        self.playing = false;
        self.playhead = 0;
        self.peak_l = 0.0;
        self.peak_r = 0.0;
        self.status = "returned to start".into();
    }

    fn toggle_playback(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.resume_or_start();
        }
    }

    /// An edit changed the project, so the pre-rendered playback buffer is now
    /// stale. Tear the session down (keeping the playhead) so the next Play
    /// re-renders with the new content.
    fn invalidate_playback(&mut self) {
        self.playback = None;
        self.playing = false;
        self.peak_l = 0.0;
        self.peak_r = 0.0;
    }

    /// Moves the playhead to an absolute frame (click-to-seek). Seeks live
    /// playback if a session is active.
    fn seek_to(&mut self, frame: u64) {
        self.playhead = frame;
        if let Some(live) = &mut self.playback {
            live.handle.seek(frame);
        }
    }

    /// Commits a clip move as an undoable command, then invalidates playback.
    fn move_clip(&mut self, track: TrackId, clip: ClipId, new_start: SampleTime) {
        let cmd = Box::new(MoveClip {
            track,
            id: clip,
            timeline_start: new_start,
        });
        self.run_edit("move clip", cmd);
    }

    /// Commits a full-geometry change (trim results) as an undoable command.
    fn set_clip_geometry(&mut self, track: TrackId, clip: ClipId, g: ClipGeom) {
        let cmd = Box::new(SetClipGeometry {
            track,
            id: clip,
            timeline_start: g.start,
            source_offset: g.offset,
            duration: g.duration,
        });
        self.run_edit("trim clip", cmd);
    }

    /// Deletes the selected clip (undoable).
    fn delete_selected(&mut self) {
        let Some((track, clip)) = self.selected else {
            self.status = "nothing selected to delete".into();
            return;
        };
        let cmd = Box::new(RemoveClip { track, id: clip });
        self.run_edit("delete clip", cmd);
        self.selected = None;
    }

    /// Splits the selected clip at the current playhead (undoable, one unit).
    fn split_selected_at_playhead(&mut self) {
        let Some((track_id, clip_id)) = self.selected else {
            self.status = "select a clip to split".into();
            return;
        };
        let at = SampleTime::new(self.playhead);

        // Compute the two resulting geometries using the model's split math,
        // without mutating the project yet.
        let Some(src) = self
            .project
            .track(track_id)
            .and_then(|t| t.clips.iter().find(|c| c.id == clip_id))
            .cloned()
        else {
            self.status = "clip not found".into();
            return;
        };
        let mut left = src.clone();
        let Some(right) = left.split_at(at) else {
            self.status = "playhead is not inside the selected clip".into();
            return;
        };

        // Apply as one undoable transaction: shrink left + add right.
        let left_geom = ClipGeom {
            start: left.timeline_start,
            offset: left.source_offset,
            duration: left.duration,
        };
        let result = self
            .history
            .transaction(&mut self.project, "split clip", |proj, tx| {
                tx.execute(
                    proj,
                    Box::new(SetClipGeometry {
                        track: track_id,
                        id: clip_id,
                        timeline_start: left_geom.start,
                        source_offset: left_geom.offset,
                        duration: left_geom.duration,
                    }),
                )?;
                tx.execute(
                    proj,
                    Box::new(AddClip {
                        track: track_id,
                        clip: right.clone(),
                    }),
                )
            });
        match result {
            Ok(()) => {
                self.status = "split clip".into();
                self.invalidate_playback();
            }
            Err(e) => self.status = format!("split failed: {e}"),
        }
    }

    /// Runs a single undoable edit command and reports/invalidates.
    fn run_edit(&mut self, what: &str, cmd: Box<dyn daw_commands::Command>) {
        match self.history.execute(&mut self.project, cmd) {
            Ok(()) => {
                self.status = what.to_string();
                self.invalidate_playback();
            }
            Err(e) => self.status = format!("{what} failed: {e}"),
        }
    }

    /// Like [`run_edit`] but does **not** tear down live playback. Used for
    /// edits whose change is baked into the pre-rendered buffer and so won't be
    /// heard until the next Play, but where restarting playback would be
    /// jarring (e.g. dragging a track-volume slider while playing). The change
    /// is still applied to the project and recorded for undo.
    ///
    /// [`run_edit`]: Self::run_edit
    fn run_edit_keep_playing(&mut self, what: &str, cmd: Box<dyn daw_commands::Command>) {
        match self.history.execute(&mut self.project, cmd) {
            Ok(()) => {
                self.status = if self.is_playing() {
                    format!("{what} (applies on next play)")
                } else {
                    what.to_string()
                };
            }
            Err(e) => self.status = format!("{what} failed: {e}"),
        }
    }

    /// Sets the live master output gain. Updates immediately on the running
    /// player (no re-render) via `SetMasterGain`; retained for future sessions.
    /// This is **not** an undoable project edit — it is a live output control
    /// (like a hardware monitor knob), separate from per-track volume.
    fn set_master_gain_live(&mut self, gain: f32) {
        self.master_gain = gain.max(0.0);
        if let Some(live) = &mut self.playback {
            live.handle.set_master_gain(self.master_gain);
        }
        self.status = format!(
            "master {:+.1} dB",
            20.0 * self.master_gain.max(1e-6).log10()
        );
    }

    /// The track whose controls the detail panel shows: the selected clip's
    /// track, or the first non-master track if nothing is selected.
    fn detail_track_id(&self) -> Option<TrackId> {
        if let Some((t, _)) = self.selected {
            return Some(t);
        }
        self.project
            .tracks
            .iter()
            .find(|t| t.kind != daw_core::track::TrackKind::Master)
            .map(|t| t.id)
    }

    fn undo(&mut self) {
        match self.history.undo(&mut self.project) {
            Ok(label) => {
                self.status = format!("undo: {label}");
                self.clear_stale_selection();
                self.invalidate_playback();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    fn redo(&mut self) {
        match self.history.redo(&mut self.project) {
            Ok(label) => {
                self.status = format!("redo: {label}");
                self.clear_stale_selection();
                self.invalidate_playback();
            }
            Err(e) => self.status = format!("{e}"),
        }
    }

    /// Drops the selection if the selected clip no longer exists (e.g. after an
    /// undo/redo that removed it).
    fn clear_stale_selection(&mut self) {
        if let Some((track, clip)) = self.selected {
            let exists = self
                .project
                .track(track)
                .is_some_and(|t| t.clips.iter().any(|c| c.id == clip));
            if !exists {
                self.selected = None;
            }
        }
    }

    /// Drains playback events to update the playhead/meters and detect the end.
    fn poll_playback(&mut self) {
        let mut reached_end = false;
        if let Some(live) = &mut self.playback {
            while let Some(ev) = live.handle.poll_event() {
                match ev {
                    daw_audio_io::Event::Meter {
                        frame,
                        peak_l,
                        peak_r,
                    } => {
                        self.playhead = frame;
                        self.peak_l = peak_l;
                        self.peak_r = peak_r;
                    }
                    daw_audio_io::Event::ReachedEnd => reached_end = true,
                    daw_audio_io::Event::Xrun(_) => {}
                }
            }
        }
        if reached_end {
            // Natural end: tear down and rewind so the next Play starts fresh.
            self.playback = None;
            self.playing = false;
            self.playhead = 0;
            self.status = "finished".into();
            self.peak_l = 0.0;
            self.peak_r = 0.0;
        }
    }

    fn device_rate(&self) -> f64 {
        self.playback
            .as_ref()
            .map_or(f64::from(self.project.sample_rate.hz()), |p| {
                f64::from(p.device_rate)
            })
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("project", &["json"])
            .pick_file()
        {
            let path = path.to_string_lossy().into_owned();
            match daw_project::load(&path) {
                Ok(p) => {
                    self.reset_to_start();
                    self.project = p;
                    self.project_path = Some(path);
                    self.history = History::new(); // fresh undo stack for the new project
                    self.status = "opened project".into();
                }
                Err(e) => self.status = format!("open failed: {e}"),
            }
        }
    }

    /// Saves the project to its current path (atomic, versioned).
    fn save(&mut self) {
        let Some(path) = self.project_path.clone() else {
            self.status = "no project path (open a project first)".into();
            return;
        };
        match daw_project::save(&self.project, &path) {
            Ok(()) => self.status = format!("saved {path}"),
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }
}

impl eframe::App for DawGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_playback();
        // Keep repainting while playing so the playhead animates.
        if self.is_playing() || self.drag.is_some() {
            ctx.request_repaint();
        }
        // Keyboard: Space toggles play/stop (a core accessibility path).
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_playback();
        }
        // Undo/redo: Cmd/Ctrl+Z, and Shift+Cmd/Ctrl+Z for redo.
        let (undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command; // Cmd on macOS, Ctrl elsewhere
            let z = i.key_pressed(egui::Key::Z);
            (
                cmd && z && !i.modifiers.shift,
                cmd && z && i.modifiers.shift,
            )
        });
        if undo {
            self.undo();
        } else if redo {
            self.redo();
        }
        // Delete/Backspace removes the selected clip; S splits at the playhead.
        if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            self.delete_selected();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::S) && !i.modifiers.command) {
            self.split_selected_at_playhead();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = theme::PANEL_BG;
        visuals.window_fill = theme::APP_BG;
        ctx.set_visuals(visuals);

        self.top_bar(ui);
        self.transport_bar(ui);
        self.status_bar(ui);
        self.detail_panel(ui);
        self.arrangement(ui);
    }
}

impl DawGui {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open project…").clicked() {
                        self.open_dialog();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.project_path.is_some(),
                            egui::Button::new("Save project"),
                        )
                        .clicked()
                    {
                        self.save();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let undo_label = self
                        .history
                        .peek_undo()
                        .map_or("Undo".to_string(), |l| format!("Undo {l}"));
                    if ui
                        .add_enabled(self.history.undo_depth() > 0, egui::Button::new(undo_label))
                        .clicked()
                    {
                        self.undo();
                        ui.close();
                    }
                    let redo_label = self
                        .history
                        .peek_redo()
                        .map_or("Redo".to_string(), |l| format!("Redo {l}"));
                    if ui
                        .add_enabled(self.history.redo_depth() > 0, egui::Button::new(redo_label))
                        .clicked()
                    {
                        self.redo();
                        ui.close();
                    }
                });
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Project: {}", self.project.name))
                        .color(theme::ACCENT),
                );
            });
        });
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        Panel::top("transport").show(ui, |ui| {
            ui.horizontal(|ui| {
                let playing = self.is_playing();

                let label = if playing { "⏸ Pause" } else { "▶ Play" };
                let resp = ui.add(egui::Button::new(label)).on_hover_text(if playing {
                    "Pause playback (Space) — keeps position"
                } else {
                    "Play / resume (Space)"
                });
                if resp.clicked() {
                    self.toggle_playback();
                }

                if ui
                    .add(egui::Button::new("⏮ Return to start"))
                    .on_hover_text("Stop and return playhead to start")
                    .clicked()
                {
                    self.reset_to_start();
                }

                ui.separator();
                let secs = self.playhead as f64 / self.device_rate();
                ui.label(format!("⏱ {secs:6.2}s"));

                // Musical position: bars:beats from playhead + tempo + time sig.
                let bpm = self.project.tempo.bpm();
                let beats_total = secs * bpm / 60.0;
                let beats_per_bar = f64::from(self.project.time_signature.numerator.max(1));
                let bar = (beats_total / beats_per_bar).floor() as u64 + 1;
                let beat = (beats_total % beats_per_bar).floor() as u64 + 1;
                ui.label(format!("{bar:03}:{beat:02}"));

                ui.separator();
                // BPM editor (undoable via SetTempo).
                let mut bpm_edit = bpm;
                ui.label("BPM");
                if ui
                    .add(
                        egui::DragValue::new(&mut bpm_edit)
                            .range(40.0..=300.0)
                            .speed(0.5),
                    )
                    .changed()
                {
                    if let Some(t) = daw_core::time::Tempo::new(bpm_edit) {
                        self.run_edit_keep_playing(
                            "set tempo",
                            Box::new(daw_commands::commands::SetTempo { tempo: t }),
                        );
                    }
                }

                ui.separator();
                let (txt, col) = if playing {
                    ("● PLAYING", theme::ACCENT)
                } else {
                    ("⏸ PAUSED", egui::Color32::GRAY)
                };
                ui.label(egui::RichText::new(txt).color(col));

                ui.separator();
                ui.label("L");
                meter_bar(ui, self.peak_l);
                ui.label("R");
                meter_bar(ui, self.peak_r);
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.status).italics());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dev = self
                        .playback
                        .as_ref()
                        .map_or("offline".to_string(), |p| format!("{} Hz", p.device_rate));
                    ui.label(format!("output: {dev}"));
                });
            });
        });
    }

    /// Bottom detail panel: controls for the selected track (volume, mute/solo),
    /// its effect chain (bypass/remove), and the master output meter. All edits
    /// route through `daw-commands` and are undoable.
    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        // Deferred edit, applied after drawing (borrow-safe).
        enum Edit {
            Volume(TrackId, Gain),
            Mute(TrackId, bool),
            Solo(TrackId, bool),
            Bypass(TrackId, EffectId, bool),
            RemoveFx(TrackId, EffectId),
            MasterGain(f32),
        }
        let mut edit: Option<Edit> = None;

        let track_id = self.detail_track_id();
        let peak_l = self.peak_l;
        let peak_r = self.peak_r;
        let mut master_gain = self.master_gain;

        Panel::bottom("detail")
            .resizable(false)
            .exact_size(176.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.columns(3, |cols| {
                    // --- column 0: track controls ---
                    let ui = &mut cols[0];
                    if let Some(tid) = track_id {
                        if let Some(track) = self.project.track(tid) {
                            ui.heading("Track");
                            ui.label(egui::RichText::new(&track.name).strong());
                            ui.add_space(6.0);

                            // Volume slider (linear 0..1.5), labeled in dB.
                            let mut v = track.volume.factor();
                            let db = if v <= 1e-6 {
                                "-inf".to_string()
                            } else {
                                format!("{:+.1} dB", 20.0 * v.log10())
                            };
                            if ui
                                .add(
                                    egui::Slider::new(&mut v, 0.0..=1.5)
                                        .text(format!("volume ({db})")),
                                )
                                .changed()
                            {
                                if let Some(g) = Gain::linear(v) {
                                    edit = Some(Edit::Volume(tid, g));
                                }
                            }

                            ui.horizontal(|ui| {
                                let mut mute = track.mute;
                                if ui.selectable_label(mute, "Mute").clicked() {
                                    mute = !mute;
                                    edit = Some(Edit::Mute(tid, mute));
                                }
                                let mut solo = track.solo;
                                if ui.selectable_label(solo, "Solo").clicked() {
                                    solo = !solo;
                                    edit = Some(Edit::Solo(tid, solo));
                                }
                            });
                        }
                    } else {
                        cols[0].weak("No track selected.");
                    }

                    // --- column 1: effect chain ---
                    let ui = &mut cols[1];
                    ui.heading("Effect chain");
                    if let Some(tid) = track_id {
                        if let Some(track) = self.project.track(tid) {
                            if track.effects.is_empty() {
                                ui.weak("No effects. (Add via the CLI for now.)");
                            }
                            for fx in &track.effects.effects {
                                ui.horizontal(|ui| {
                                    ui.label(&fx.processor_type);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("✕").on_hover_text("Remove").clicked() {
                                                edit = Some(Edit::RemoveFx(tid, fx.id));
                                            }
                                            let mut bypassed = fx.bypassed;
                                            if ui.selectable_label(bypassed, "Bypass").clicked() {
                                                bypassed = !bypassed;
                                                edit = Some(Edit::Bypass(tid, fx.id, bypassed));
                                            }
                                        },
                                    );
                                });
                            }
                        }
                    }

                    // --- column 2: output meter ---
                    let ui = &mut cols[2];
                    ui.heading("Output meter");
                    ui.horizontal(|ui| {
                        ui.label("L");
                        meter_bar(ui, peak_l);
                    });
                    ui.horizontal(|ui| {
                        ui.label("R");
                        meter_bar(ui, peak_r);
                    });
                    ui.add_space(4.0);
                    let dev = self.playback.as_ref().map_or_else(
                        || format!("{} Hz (offline)", self.project.sample_rate.hz()),
                        |p| format!("{} Hz · stereo", p.device_rate),
                    );
                    ui.weak(dev);

                    ui.add_space(6.0);
                    // Live master output gain — applies instantly during
                    // playback (no re-render), unlike per-track volume.
                    let db = if master_gain <= 1e-6 {
                        "-inf".to_string()
                    } else {
                        format!("{:+.1} dB", 20.0 * master_gain.log10())
                    };
                    if ui
                        .add(
                            egui::Slider::new(&mut master_gain, 0.0..=1.5)
                                .text(format!("master ({db})")),
                        )
                        .on_hover_text("Live master output — changes are heard immediately")
                        .changed()
                    {
                        edit = Some(Edit::MasterGain(master_gain));
                    }
                });
                ui.add_space(6.0);
            });

        // Apply the collected edit via the command/undo layer.
        if let Some(e) = edit {
            match e {
                Edit::Volume(t, g) => self.run_edit_keep_playing(
                    "set volume",
                    Box::new(SetTrackVolume { id: t, volume: g }),
                ),
                Edit::Mute(t, m) => self
                    .run_edit_keep_playing("set mute", Box::new(SetTrackMute { id: t, mute: m })),
                Edit::Solo(t, s) => self
                    .run_edit_keep_playing("set solo", Box::new(SetTrackSolo { id: t, solo: s })),
                Edit::Bypass(t, fx, b) => self.run_edit_keep_playing(
                    "bypass effect",
                    Box::new(SetEffectBypass {
                        track: t,
                        id: fx,
                        bypassed: b,
                    }),
                ),
                Edit::RemoveFx(t, fx) => self.remove_effect_with_automation(t, fx),
                Edit::MasterGain(g) => self.set_master_gain_live(g),
            }
        }
    }

    /// Removes an effect and any automation lanes that target it, as one
    /// undoable transaction (so the project stays valid — otherwise the lanes
    /// would dangle and validation would reject the removal).
    fn remove_effect_with_automation(&mut self, track: TrackId, effect: EffectId) {
        use daw_commands::commands::{RemoveAutomationLane, RemoveEffect};

        // Collect the targets of lanes controlling this effect (clone them so we
        // don't borrow the project while editing).
        let lane_targets: Vec<_> = self
            .project
            .automation_lanes_for_effect(track, effect)
            .into_iter()
            .filter_map(|i| self.project.automation.get(i).map(|l| l.target.clone()))
            .collect();

        let result = self
            .history
            .transaction(&mut self.project, "remove effect", |proj, tx| {
                for target in &lane_targets {
                    tx.execute(
                        proj,
                        Box::new(RemoveAutomationLane {
                            target: target.clone(),
                        }),
                    )?;
                }
                tx.execute(proj, Box::new(RemoveEffect { track, id: effect }))
            });
        match result {
            Ok(()) => {
                let n = lane_targets.len();
                self.status = if n > 0 {
                    format!("removed effect (+{n} automation lane(s))")
                } else {
                    "removed effect".into()
                };
                self.invalidate_playback();
            }
            Err(e) => self.status = format!("remove effect failed: {e}"),
        }
    }

    fn arrangement(&mut self, ui: &mut egui::Ui) {
        // Deferred edits/seeks collected during drawing, applied after.
        enum Action {
            Seek(u64),
            Select(TrackId, ClipId, Option<u64>), // select a clip; Some(frame) also seeks
            StartDrag(ClipDrag),
            DragTo(ClipGeom),
            EndDrag,
        }
        let mut action: Option<Action> = None;

        let rate = f64::from(self.project.sample_rate.hz().max(1));
        let px_per_sec = self.px_per_sec;
        let device_rate = self.device_rate();
        let playhead = self.playhead;
        let drag_active = self.drag.is_some();
        let selected = self.selected;

        // frames <-> pixels helpers (lane-relative handled per-lane via left edge).
        let dx_to_frames = |dx: f32| -> i64 { (dx as f64 / px_per_sec as f64 * rate) as i64 };

        CentralPanel::default().show(ui, |ui| {
            ui.heading("Arrangement");
            ui.weak(
                "Ruler: click/drag to seek · click clip: select · double-click clip: seek · \
                 drag body: move · drag edges: trim · S: split · Del: delete · Cmd/Ctrl+Z: undo",
            );
            ui.add_space(4.0);

            // Dedicated seek ruler lane: a header spacer + a clickable/draggable
            // timeline ruler that moves the playhead.
            {
                let ruler_h = 22.0;
                ui.horizontal(|ui| {
                    ui.allocate_exact_size(egui::vec2(140.0, ruler_h), egui::Sense::hover());
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(200.0), ruler_h),
                        egui::Sense::click_and_drag(),
                    );
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 2.0, theme::APP_BG);

                    let x_to_frame = |x: f32| -> u64 {
                        let secs = ((x - rect.left()).max(0.0) / px_per_sec) as f64;
                        (secs * rate).round() as u64
                    };

                    let visible_secs = (rect.width() / px_per_sec).ceil() as i32;
                    for s in 0..=visible_secs {
                        let x = rect.left() + s as f32 * px_per_sec;
                        if x > rect.right() {
                            break;
                        }
                        painter.vline(
                            x,
                            egui::Rangef::new(rect.bottom() - 6.0, rect.bottom()),
                            egui::Stroke::new(1.0, egui::Color32::GRAY),
                        );
                        painter.text(
                            egui::pos2(x + 2.0, rect.top() + 1.0),
                            egui::Align2::LEFT_TOP,
                            format!("{s}s"),
                            egui::FontId::monospace(9.0),
                            egui::Color32::GRAY,
                        );
                    }

                    if resp.clicked() || resp.dragged() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            action = Some(Action::Seek(x_to_frame(pos.x)));
                        }
                    }

                    let secs = playhead as f64 / device_rate;
                    let px = rect.left() + (secs as f32) * px_per_sec;
                    if px <= rect.right() {
                        painter.vline(px, rect.y_range(), egui::Stroke::new(1.5, theme::DANGER));
                    }
                });
                ui.add_space(2.0);
            }

            egui::ScrollArea::both().show(ui, |ui| {
                let only_master = self
                    .project
                    .tracks
                    .iter()
                    .all(|t| t.kind == TrackKind::Master);

                for track in &self.project.tracks {
                    if track.kind == TrackKind::Master {
                        continue;
                    }
                    let track_id = track.id;
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(140.0, 48.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let kind = match track.kind {
                                    TrackKind::Audio => "audio",
                                    TrackKind::Midi => "midi",
                                    TrackKind::Bus => "bus",
                                    TrackKind::Master => "master",
                                };
                                ui.label(egui::RichText::new(&track.name).strong());
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{kind} · {} fx",
                                        track.effects.len()
                                    ))
                                    .weak()
                                    .small(),
                                );
                            },
                        );

                        let lane_h = 48.0;
                        let (rect, _lane_resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width().max(200.0), lane_h),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 2.0, theme::PANEL_BG);

                        let x_to_frame = |x: f32| -> u64 {
                            let secs = ((x - rect.left()).max(0.0) / px_per_sec) as f64;
                            (secs * rate).round() as u64
                        };

                        for clip in &track.clips {
                            let clip_id = clip.id;
                            let is_selected = selected == Some((track_id, clip_id));

                            // Live preview geometry if this clip is being dragged.
                            let (gstart, gdur) = if let Some(d) = &self.drag {
                                if d.clip == clip_id {
                                    let g = d.preview.unwrap_or(ClipGeom {
                                        start: d.original_start,
                                        offset: d.original_offset,
                                        duration: d.original_duration,
                                    });
                                    (g.start.frames(), g.duration.frames())
                                } else {
                                    (clip.timeline_start.frames(), clip.duration.frames())
                                }
                            } else {
                                (clip.timeline_start.frames(), clip.duration.frames())
                            };

                            let x0 = rect.left() + (gstart as f64 / rate) as f32 * px_per_sec;
                            let w = ((gdur as f64 / rate) as f32 * px_per_sec).max(4.0);
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(x0, rect.top() + 4.0),
                                egui::vec2(w, lane_h - 8.0),
                            );

                            let id = egui::Id::new(("clip", clip_id.as_uuid()));
                            let resp = ui.interact(clip_rect, id, egui::Sense::click_and_drag());

                            // Edge zones (8px) for trim; otherwise move.
                            let edge = 8.0_f32.min(w / 3.0);
                            let hover_mode = resp.hover_pos().map(|p| {
                                if p.x <= clip_rect.left() + edge {
                                    DragMode::TrimStart
                                } else if p.x >= clip_rect.right() - edge {
                                    DragMode::TrimEnd
                                } else {
                                    DragMode::Move
                                }
                            });
                            if matches!(hover_mode, Some(DragMode::TrimStart | DragMode::TrimEnd)) {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            }

                            let fill = if is_selected {
                                theme::ACCENT.gamma_multiply(0.6)
                            } else if resp.hovered() || resp.dragged() {
                                theme::ACCENT.gamma_multiply(0.4)
                            } else {
                                theme::CLIP
                            };
                            painter.rect_filled(clip_rect, 3.0, fill);
                            let stroke_w = if is_selected { 2.0 } else { 1.0 };
                            painter.rect_stroke(
                                clip_rect,
                                3.0,
                                egui::Stroke::new(stroke_w, theme::ACCENT),
                                egui::StrokeKind::Inside,
                            );

                            if resp.double_clicked() {
                                // Double-click: select and seek to the clicked position.
                                let frame = resp
                                    .interact_pointer_pos()
                                    .map_or(gstart, |p| x_to_frame(p.x));
                                action = Some(Action::Select(track_id, clip_id, Some(frame)));
                            } else if resp.clicked() {
                                // Single click: select only.
                                action = Some(Action::Select(track_id, clip_id, None));
                            }

                            // Drag lifecycle.
                            if resp.drag_started() {
                                if let Some(pos) = resp.interact_pointer_pos() {
                                    let mode = hover_mode.unwrap_or(DragMode::Move);
                                    action = Some(Action::StartDrag(ClipDrag {
                                        track: track_id,
                                        clip: clip_id,
                                        mode,
                                        original_start: clip.timeline_start,
                                        original_offset: clip.source_offset,
                                        original_duration: clip.duration,
                                        grab_x: pos.x,
                                        preview: None,
                                    }));
                                }
                            } else if resp.dragged() && drag_active {
                                if let Some(pos) = resp.interact_pointer_pos() {
                                    if let Some(d) = &self.drag {
                                        if d.clip == clip_id {
                                            let delta = dx_to_frames(pos.x - d.grab_x);
                                            action =
                                                Some(Action::DragTo(compute_drag_geom(d, delta)));
                                        }
                                    }
                                }
                            } else if resp.drag_stopped() {
                                action = Some(Action::EndDrag);
                            }
                        }

                        // Playhead line.
                        let secs = playhead as f64 / device_rate;
                        let px = rect.left() + (secs as f32) * px_per_sec;
                        if px <= rect.right() {
                            painter.vline(
                                px,
                                rect.y_range(),
                                egui::Stroke::new(1.5, theme::DANGER),
                            );
                        }
                    });
                    ui.add_space(4.0);
                }

                if only_master {
                    ui.weak("No tracks yet. Build a project with the CLI, then File → Open.");
                }
            });
        });

        // Apply the collected action after drawing.
        match action {
            Some(Action::Seek(frame)) => self.seek_to(frame),
            Some(Action::Select(t, c, seek)) => {
                self.selected = Some((t, c));
                if let Some(frame) = seek {
                    self.seek_to(frame);
                }
            }
            Some(Action::StartDrag(d)) => {
                self.selected = Some((d.track, d.clip));
                self.drag = Some(d);
            }
            Some(Action::DragTo(geom)) => {
                if let Some(d) = &mut self.drag {
                    d.preview = Some(geom);
                }
            }
            Some(Action::EndDrag) => {
                if let Some(d) = self.drag.take() {
                    if let Some(g) = d.preview {
                        let orig = ClipGeom {
                            start: d.original_start,
                            offset: d.original_offset,
                            duration: d.original_duration,
                        };
                        if g != orig {
                            match d.mode {
                                DragMode::Move => self.move_clip(d.track, d.clip, g.start),
                                DragMode::TrimStart | DragMode::TrimEnd => {
                                    self.set_clip_geometry(d.track, d.clip, g)
                                }
                            }
                        }
                    }
                }
            }
            None => {}
        }
    }
}

/// Computes the previewed clip geometry for a drag of `delta` frames in the
/// drag's mode. Trims are clamped so the clip keeps at least 1 frame and the
/// source offset never goes negative.
fn compute_drag_geom(d: &ClipDrag, delta: i64) -> ClipGeom {
    let start = d.original_start.frames() as i64;
    let offset = d.original_offset.frames() as i64;
    let dur = d.original_duration.frames() as i64;

    match d.mode {
        DragMode::Move => ClipGeom {
            start: SampleTime::new((start + delta).max(0) as u64),
            offset: d.original_offset,
            duration: d.original_duration,
        },
        DragMode::TrimStart => {
            // Move the start edge: start and offset move together, duration
            // shrinks/grows oppositely. Clamp so offset >= 0 and duration >= 1,
            // and start can't pass the fixed right edge.
            let max_right_delta = dur - 1; // can't shrink below 1 frame
            let min_left_delta = -(offset.min(start)); // offset/start can't go < 0
            let d2 = delta.clamp(min_left_delta, max_right_delta);
            ClipGeom {
                start: SampleTime::new((start + d2) as u64),
                offset: SampleTime::new((offset + d2) as u64),
                duration: SampleTime::new((dur - d2) as u64),
            }
        }
        DragMode::TrimEnd => {
            // Move the right edge: duration only. Keep >= 1 frame.
            let new_dur = (dur + delta).max(1);
            ClipGeom {
                start: d.original_start,
                offset: d.original_offset,
                duration: SampleTime::new(new_dur as u64),
            }
        }
    }
}

/// Draws a small horizontal peak meter (0..1, clamped).
fn meter_bar(ui: &mut egui::Ui, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 12.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, theme::PANEL_BG);
    let w = rect.width() * peak.clamp(0.0, 1.0);
    let fill = egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height()));
    let col = if peak >= 0.99 {
        theme::DANGER
    } else {
        theme::ACCENT
    };
    p.rect_filled(fill, 2.0, col);
}

/// Launches the GUI event loop with `project` (blocking until the window
/// closes). Returns an error string if the window system is unavailable.
pub fn run(project: Project, project_path: Option<String>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Free Sound (prototype)",
        options,
        Box::new(move |_cc| Ok(Box::new(DawGui::new(project, project_path)))),
    )
    .map_err(|e| format!("GUI failed to start: {e}"))
}
