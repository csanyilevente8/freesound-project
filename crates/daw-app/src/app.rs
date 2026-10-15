//! Application shell logic, decoupled from any concrete UI framework.
//!
//! Milestone 0 deliberately does not commit to a GUI toolkit (ADR-0002 is
//! open). Instead, the application state and the operations the UI needs are
//! modeled here behind a small, framework-agnostic surface. A future egui (or
//! other) frontend becomes a thin presenter over [`AppState`] without touching
//! the core crates. This keeps the GUI replaceable (spec principle §2.1).

use daw_core::project::Project;
use daw_core::time::SampleRate;
use daw_core::track::TrackKind;

/// A demonstration asset source producing a fixed-frequency sine tone.
///
/// Milestone 1 has no audio-file decoder yet (deferred with the import
/// workflow). To make offline export reachable from the shell, this synthetic
/// source stands in for decoded media: it generates a sine wave on demand for
/// any asset id. It implements [`daw_engine::schedule::AssetSource`].
pub struct SineSource {
    /// Frequency in Hz.
    pub freq: f32,
    /// Sample rate the tone is generated at.
    pub sample_rate: f32,
    /// Peak amplitude in [0, 1].
    pub amplitude: f32,
}

impl daw_engine::schedule::AssetSource for SineSource {
    fn read(
        &self,
        _asset: daw_core::id::AssetId,
        _channel: usize,
        start: daw_core::time::SampleTime,
        out: &mut [f32],
    ) -> usize {
        let two_pi = std::f32::consts::TAU;
        for (i, s) in out.iter_mut().enumerate() {
            let n = start.frames() as f32 + i as f32;
            *s = self.amplitude * (two_pi * self.freq * n / self.sample_rate).sin();
        }
        out.len()
    }
}

/// Minimal application state: the currently open project.
///
/// This is where selection, panel layout, and other UI-independent-yet-app
/// level state will live as the shell grows. For now it holds the project.
pub struct AppState {
    /// The currently open project.
    pub project: Project,
}

impl AppState {
    /// Creates application state with a fresh empty project.
    #[must_use]
    pub fn with_new_project(name: &str) -> Self {
        Self {
            project: Project::new(name, SampleRate::default()),
        }
    }

    /// Creates a demo project containing one audio track with a single clip,
    /// backed conceptually by a synthetic tone asset. Used by the shell's
    /// `--export` demonstration so the offline export path has content.
    ///
    /// The returned [`SineSource`] generates the audio for the clip's asset.
    #[must_use]
    pub fn demo_project(name: &str) -> (Self, SineSource) {
        use daw_core::audio::ChannelLayout;
        use daw_core::clip::AudioClip;
        use daw_core::media::MediaAssetRef;
        use daw_core::time::SampleTime;
        use daw_core::track::Track;

        let rate = SampleRate::default();
        let mut project = Project::new(name, rate);
        let one_second = SampleTime::new(u64::from(rate.hz()));
        let asset = project.add_asset(MediaAssetRef::new(
            "<synthetic sine tone>",
            rate,
            ChannelLayout::Stereo,
            one_second,
        ));
        let mut track = Track::new(TrackKind::Audio, "Tone");
        track.add_clip(AudioClip::new(asset, SampleTime::ZERO, one_second));
        project.add_track(track);

        let source = SineSource {
            freq: 440.0,
            sample_rate: rate.hz() as f32,
            amplitude: 0.5,
        };
        (Self { project }, source)
    }

    /// Renders a human-readable summary of the open project.
    ///
    /// This is the stand-in for a graphical arrangement view: it proves the
    /// shell can open and display a project without claiming playback or
    /// recording exist.
    #[must_use]
    pub fn describe(&self) -> String {
        let p = &self.project;
        let mut out = String::new();
        out.push_str(&format!("Project: {}\n", p.name));
        out.push_str(&format!("  id:            {}\n", p.id));
        out.push_str(&format!("  sample rate:   {} Hz\n", p.sample_rate.hz()));
        out.push_str(&format!("  tempo:         {} bpm\n", p.tempo.bpm()));
        out.push_str(&format!(
            "  time sig:      {}/{}\n",
            p.time_signature.numerator, p.time_signature.denominator
        ));
        out.push_str(&format!("  tracks:        {}\n", p.tracks.len()));
        out.push_str(&format!("  assets:        {}\n", p.assets.len()));
        for (i, t) in p.tracks.iter().enumerate() {
            let kind = match t.kind {
                TrackKind::Audio => "audio",
                TrackKind::Midi => "midi",
                TrackKind::Bus => "bus",
                TrackKind::Master => "master",
            };
            out.push_str(&format!(
                "    [{i}] {kind:<6} \"{}\" ({} clip(s))\n",
                t.name,
                t.clips.len()
            ));
        }
        out.push_str(
            "\nStatus: no audio device configured (offline). \
             Live playback and recording are not implemented yet; offline \
             export is available.\n",
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_shell_has_master_track() {
        let app = AppState::with_new_project("Untitled");
        assert_eq!(app.project.tracks.len(), 1);
        assert!(app.project.master().is_some());
    }

    #[test]
    fn describe_mentions_project_and_offline_status() {
        let app = AppState::with_new_project("Demo");
        let text = app.describe();
        assert!(text.contains("Project: Demo"));
        assert!(text.contains("master"));
        assert!(text.contains("offline"));
    }
}
