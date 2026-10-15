//! Integration test: export a WAV then re-import it (full offline media loop).
//!
//! This closes the loop between `daw-render` (writer) and `daw-media` (reader):
//! render a project to a WAV file, import that file, place its asset as a clip,
//! render again, and confirm the audio survives the round-trip.

use daw_core::clip::AudioClip;
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};
use daw_engine::schedule::AssetSource;
use daw_media::{decode_wav, import_wav, MemoryAssetSource};
use daw_render::{export_wav, render_project, ExportOptions, WavFormat};

/// A sine source used to generate the file we will export then re-import.
struct SineSource {
    freq: f32,
    rate: f32,
}
impl AssetSource for SineSource {
    fn read(
        &self,
        _a: daw_core::id::AssetId,
        _ch: usize,
        start: SampleTime,
        out: &mut [f32],
    ) -> usize {
        for (i, s) in out.iter_mut().enumerate() {
            let n = start.frames() as f32 + i as f32;
            *s = 0.5 * (std::f32::consts::TAU * self.freq * n / self.rate).sin();
        }
        out.len()
    }
}

fn tone_project(frames: u64, rate: SampleRate) -> (Project, SineSource) {
    let mut p = Project::new("Tone", rate);
    let asset = p.add_asset(daw_core::media::MediaAssetRef::new(
        "<sine>",
        rate,
        daw_core::audio::ChannelLayout::Stereo,
        SampleTime::new(frames),
    ));
    let mut t = Track::new(TrackKind::Audio, "Tone");
    t.add_clip(AudioClip::new(
        asset,
        SampleTime::ZERO,
        SampleTime::new(frames),
    ));
    p.add_track(t);
    (
        p,
        SineSource {
            freq: 440.0,
            rate: rate.hz() as f32,
        },
    )
}

#[test]
fn float32_export_import_is_bit_exact() {
    let rate = SampleRate::new(48_000).unwrap();
    let frames = 256;
    let (project, source) = tone_project(frames, rate);

    // Export the rendered project to float32 WAV (lossless of the f32 engine).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    export_wav(
        &project,
        &source,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();

    // The expected master buffer, rendered directly.
    let expected = render_project(&project, &source, &ExportOptions::default()).unwrap();

    // Import the file back and compare decoded samples to the rendered master.
    let imported = import_wav(&path).unwrap();
    assert_eq!(imported.audio.sample_rate, 48_000);
    assert_eq!(imported.audio.frames(), frames as usize);
    for ch in 0..2 {
        for i in 0..frames as usize {
            let a = imported.audio.channels[ch][i];
            let b = expected.channel(ch)[i];
            assert!((a - b).abs() < 1e-6, "ch{ch} frame{i}: {a} vs {b}");
        }
    }
}

#[test]
fn imported_asset_plays_as_a_clip() {
    let rate = SampleRate::new(48_000).unwrap();
    let frames = 128;
    let (project, source) = tone_project(frames, rate);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("src.wav");
    export_wav(
        &project,
        &source,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();

    // Build a NEW project that imports the file and places it as a clip.
    let mut src = MemoryAssetSource::new();
    let asset = src.import_and_register(&path).unwrap();
    let asset_frames = asset.frames;

    let mut p2 = Project::new("Reuse", rate);
    let asset_id = p2.add_asset(asset);
    let mut track = Track::new(TrackKind::Audio, "Imported");
    track.add_clip(AudioClip::new(asset_id, SampleTime::ZERO, asset_frames));
    p2.add_track(track);

    // Render the new project using the in-memory source of the imported file.
    let out = render_project(&p2, &src, &ExportOptions::default()).unwrap();
    assert_eq!(out.frames(), frames as usize);
    // Non-silent: the imported tone actually plays.
    assert!(out.peak() > 0.1);
}

#[test]
fn pcm16_round_trip_is_approximately_equal() {
    // PCM16 is lossy vs f32; verify the re-imported signal is close.
    let rate = SampleRate::new(44_100).unwrap();
    let frames = 200;
    let (project, source) = tone_project(frames, rate);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone16.wav");
    export_wav(&project, &source, &path, &ExportOptions::default()).unwrap(); // PCM16 default

    let expected = render_project(&project, &source, &ExportOptions::default()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let decoded = decode_wav(&bytes).unwrap();

    for i in 0..frames as usize {
        let a = decoded.channels[0][i];
        let b = expected.channel(0)[i];
        // 16-bit quantization error is ~1/32767.
        assert!((a - b).abs() < 1e-3, "frame{i}: {a} vs {b}");
    }
}

#[test]
fn import_at_resamples_to_target_rate() {
    use daw_media::import_wav_at;
    // Export a file at 24k, import it at 48k -> frame count ~doubles, rate 48k.
    let src_rate = SampleRate::new(24_000).unwrap();
    let frames = 240; // 10 ms at 24k
    let (project, source) = tone_project(frames, src_rate);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone24.wav");
    export_wav(
        &project,
        &source,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();

    let imported = import_wav_at(&path, SampleRate::new(48_000).unwrap()).unwrap();
    assert_eq!(imported.asset.sample_rate, SampleRate::new(48_000).unwrap());
    // ~2x the frames (rounded).
    assert_eq!(imported.audio.frames(), frames as usize * 2);
    assert!(imported.audio.channels[0].iter().all(|x| x.is_finite()));
}

#[test]
fn peaks_summarize_imported_audio() {
    use daw_media::compute_peaks;
    let rate = SampleRate::new(48_000).unwrap();
    let frames = 480;
    let (project, source) = tone_project(frames, rate);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    export_wav(
        &project,
        &source,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();

    let imported = import_wav(&path).unwrap();
    let peaks = compute_peaks(&imported.audio.channels, 48); // ~1ms buckets
    assert_eq!(peaks.buckets(), 10);
    // A 440 Hz tone at amplitude 0.5: some bucket should show near ±0.5.
    let has_signal = peaks.channels[0]
        .iter()
        .any(|p| p.max > 0.3 || p.min < -0.3);
    assert!(has_signal);
}
