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

use daw_core::project::Project;
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
                Ok(live) => {
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
                    self.status = "opened project".into();
                }
                Err(e) => self.status = format!("open failed: {e}"),
            }
        }
    }
}

impl eframe::App for DawGui {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_playback();
        // Keep repainting while playing so the playhead animates.
        if self.is_playing() {
            ctx.request_repaint();
        }
        // Keyboard: Space toggles play/stop (a core accessibility path).
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_playback();
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

    fn arrangement(&mut self, ui: &mut egui::Ui) {
        CentralPanel::default().show(ui, |ui| {
            ui.heading("Arrangement");
            ui.add_space(4.0);

            let rate = f64::from(self.project.sample_rate.hz().max(1));
            let px_per_sec = self.px_per_sec;
            let device_rate = self.device_rate();
            let playhead = self.playhead;

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
                        let (rect, _resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width().max(200.0), lane_h),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 2.0, theme::PANEL_BG);

                        for clip in &track.clips {
                            let x0 = rect.left()
                                + (clip.timeline_start.frames() as f64 / rate) as f32 * px_per_sec;
                            let w = (clip.duration.frames() as f64 / rate) as f32 * px_per_sec;
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(x0, rect.top() + 4.0),
                                egui::vec2(w.max(2.0), lane_h - 8.0),
                            );
                            painter.rect_filled(clip_rect, 3.0, theme::CLIP);
                            painter.rect_stroke(
                                clip_rect,
                                3.0,
                                egui::Stroke::new(1.0, theme::ACCENT),
                                egui::StrokeKind::Inside,
                            );
                        }

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
