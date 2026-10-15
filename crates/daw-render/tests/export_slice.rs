//! Integration test: Milestone 1 export vertical slice.
//!
//! Builds a project with an audio clip, saves and reloads it (exercising the
//! v2 schema), then renders and exports it to a WAV file — all offline, with a
//! synthetic asset source. Verifies the clip audio reaches the output and the
//! file is a well-formed WAV.

use daw_core::audio::ChannelLayout;
use daw_core::clip::AudioClip;
use daw_core::id::AssetId;
use daw_core::media::MediaAssetRef;
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};
use daw_engine::schedule::AssetSource;
use daw_render::{export_wav, render_project, ExportOptions, WavFormat};

/// Synthetic source: constant value for the whole asset.
struct ConstSource {
    value: f32,
    len: u64,
}
impl AssetSource for ConstSource {
    fn read(&self, _a: AssetId, _ch: usize, start: SampleTime, out: &mut [f32]) -> usize {
        let avail = self.len.saturating_sub(start.frames()) as usize;
        let n = avail.min(out.len());
        for s in out.iter_mut().take(n) {
            *s = self.value;
        }
        n
    }
}

fn build() -> (Project, ConstSource) {
    let rate = SampleRate::new(48_000).unwrap();
    let mut p = Project::new("M1 Slice", rate);
    let len = 480; // 10 ms at 48k
    let asset = p.add_asset(MediaAssetRef::new(
        "media/audio/take.wav",
        rate,
        ChannelLayout::Stereo,
        SampleTime::new(len),
    ));
    let mut track = Track::new(TrackKind::Audio, "Guitar");
    track.add_clip(AudioClip::new(
        asset,
        SampleTime::ZERO,
        SampleTime::new(len),
    ));
    p.add_track(track);
    (p, ConstSource { value: 0.5, len })
}

#[test]
fn save_reload_then_export_is_consistent() {
    let (project, src) = build();

    // Save/reload exercises the v2 document + validation (clip references asset).
    let json = daw_project::to_json(&project).unwrap();
    let reloaded = daw_project::from_json(&json).unwrap();
    assert_eq!(project, reloaded);

    let before = render_project(&reloaded, &src, &ExportOptions::default()).unwrap();
    assert_eq!(before.frames(), 480);
    assert!((before.channel(0)[0] - 0.5).abs() < 1e-6);

    // Export to a WAV file and sanity-check it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mix.wav");
    export_wav(
        &reloaded,
        &src,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    // float32 stereo: 44-byte header + 480 * 2 * 4 = 44 + 3840 = 3884 bytes.
    assert_eq!(bytes.len(), 3884);
    // First float sample equals the rendered level (0.5).
    let first = f32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);
    assert!((first - 0.5).abs() < 1e-6);
}
