//! A small subcommand CLI for driving offline DAW sessions from the terminal.
//!
//! This is a **manual-testing / headless** front end, not the eventual GUI
//! (ADR-0002). It operates on a project JSON file on disk, building up a real
//! session incrementally: create a project, import audio, add tracks/clips/
//! effects/automation, inspect state, and export a WAV.
//!
//! All edits go through the same `daw-core` model and `daw-project`
//! persistence used everywhere else; media is referenced by path (never
//! copied/rewritten) and re-decoded on export, so the workflow stays
//! non-destructive. Decoded samples are not stored in the project file.

use std::path::Path;

use daw_core::audio::{ChannelLayout, Gain};
use daw_core::automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation};
use daw_core::clip::AudioClip;
use daw_core::effect::{EffectInstance, ParamValue};
use daw_core::id::{EffectId, TrackId};
use daw_core::media::MediaAssetRef;
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};
use daw_core::Validate;
use daw_media::MemoryAssetSource;
use daw_render::{ExportOptions, WavFormat};

/// Result of running a CLI command: lines to print on success, or an error.
pub type CliResult = Result<String, String>;

/// Entry point: dispatch on the first argument (the subcommand).
///
/// `args` excludes the program name (i.e. `std::env::args().skip(1)`).
pub fn run(args: &[String]) -> CliResult {
    let (cmd, rest) = args.split_first().ok_or_else(|| help_text().to_string())?;

    match cmd.as_str() {
        "help" | "-h" | "--help" => Ok(help_text().to_string()),
        "new" => cmd_new(rest),
        "show" => cmd_show(rest),
        "import" => cmd_import(rest),
        "add-track" => cmd_add_track(rest),
        "add-clip" => cmd_add_clip(rest),
        "add-effect" => cmd_add_effect(rest),
        "automate" => cmd_automate(rest),
        "set-volume" => cmd_set_volume(rest),
        "export" => cmd_export(rest),
        other => Err(format!("unknown command '{other}'\n\n{}", help_text())),
    }
}

fn help_text() -> &'static str {
    "Free Sound — offline CLI (manual testing)\n\
     \n\
     Usage: daw <command> [args]\n\
     \n\
     Commands:\n\
     \u{20}  new <proj.json> [name]                 Create a new project file\n\
     \u{20}  show <proj.json>                       Print the full project state\n\
     \u{20}  import <proj.json> <file.wav>          Import a WAV as a media asset\n\
     \u{20}  add-track <proj.json> <name> [audio|midi|bus]  Add a track\n\
     \u{20}  add-clip <proj.json> <track> <asset> [start_frames] [dur_frames]\n\
     \u{20}  add-effect <proj.json> <track> <type> [k=v ...]  Add an insert effect\n\
     \u{20}  automate <proj.json> <track> <target> <t:v> [t:v ...]  Automate a parameter\n\
     \u{20}      target = volume | pan | effect:<id-or-type>:<param>\n\
     \u{20}      e.g. a wah:  automate p.json Gtr effect:filter:cutoff 0:300 24000:3000 48000:300\n\
     \u{20}  set-volume <proj.json> <track> <linear_gain>  Set a track's volume\n\
     \u{20}  export <proj.json> <out.wav> [pcm16|float32]   Render to a WAV\n\
     \n\
     <track> and <asset> accept either the entity's name (tracks) or its id.\n\
     Effect types: gain, pan, distortion, filter, delay, reverb, eq, compressor.\n\
     Example params: drive=5 output=0.8  |  cutoff=800 resonance=0.6\n"
}

// --- helpers ---------------------------------------------------------------

fn load(path: &str) -> Result<Project, String> {
    daw_project::load(path).map_err(|e| format!("failed to load {path}: {e}"))
}

fn store(project: &Project, path: &str) -> Result<(), String> {
    daw_project::save(project, path).map_err(|e| format!("failed to save {path}: {e}"))
}

/// Resolves a track by name or id string.
fn resolve_track(project: &Project, needle: &str) -> Result<TrackId, String> {
    if let Some(t) = project.tracks.iter().find(|t| t.name == needle) {
        return Ok(t.id);
    }
    if let Ok(uuid) = needle.parse() {
        let id = TrackId::from_uuid(uuid);
        if project.track(id).is_some() {
            return Ok(id);
        }
    }
    Err(format!("no track matching '{needle}'"))
}

/// Resolves an asset by (file-name match on) path or id string.
fn resolve_asset(project: &Project, needle: &str) -> Result<daw_core::id::AssetId, String> {
    if let Ok(uuid) = needle.parse() {
        let id = daw_core::id::AssetId::from_uuid(uuid);
        if project.asset(id).is_some() {
            return Ok(id);
        }
    }
    // Match on exact path or trailing file name.
    if let Some(a) = project
        .assets
        .iter()
        .find(|a| a.path == needle || a.path.ends_with(needle))
    {
        return Ok(a.id);
    }
    Err(format!("no asset matching '{needle}'"))
}

/// Resolves an effect on `track` by id string or by processor type (first match).
fn resolve_effect(project: &Project, track: TrackId, needle: &str) -> Result<EffectId, String> {
    let t = project.track(track).ok_or("track not found")?;
    if let Ok(uuid) = needle.parse() {
        let id = EffectId::from_uuid(uuid);
        if t.effects.get(id).is_some() {
            return Ok(id);
        }
    }
    if let Some(e) = t
        .effects
        .effects
        .iter()
        .find(|e| e.processor_type == needle)
    {
        return Ok(e.id);
    }
    Err(format!("no effect matching '{needle}' on the track"))
}

fn track_kind(s: &str) -> TrackKind {
    match s {
        "midi" => TrackKind::Midi,
        "bus" => TrackKind::Bus,
        _ => TrackKind::Audio,
    }
}

/// Parses `k=v` effect params into a `ParamValue` map applied to an instance.
fn apply_params(mut effect: EffectInstance, pairs: &[String]) -> Result<EffectInstance, String> {
    for pair in pairs {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| format!("param '{pair}' must be key=value"))?;
        let value = if let Ok(i) = v.parse::<i64>() {
            // Prefer float for known continuous params; keep int only for "mode"/"type".
            if k == "mode" || k == "type" {
                ParamValue::Int(i)
            } else {
                ParamValue::Float(i as f32)
            }
        } else if let Ok(f) = v.parse::<f32>() {
            ParamValue::Float(f)
        } else if let Ok(b) = v.parse::<bool>() {
            ParamValue::Bool(b)
        } else {
            return Err(format!("param '{k}' has unparseable value '{v}'"));
        };
        effect.set_param(k, value);
    }
    Ok(effect)
}

/// Builds a `MemoryAssetSource` by importing every asset's stored path.
fn source_for(project: &Project) -> Result<MemoryAssetSource, String> {
    let mut src = MemoryAssetSource::new();
    for asset in &project.assets {
        // Re-import at the project rate (resampling if needed).
        let imported = daw_media::import_wav_at(&asset.path, project.sample_rate)
            .map_err(|e| format!("failed to re-import asset {}: {e}", asset.path))?;
        // Register under the project's asset id (not the freshly generated one).
        src.insert(asset.id, imported.audio);
    }
    Ok(src)
}

// --- commands --------------------------------------------------------------

fn cmd_new(args: &[String]) -> CliResult {
    let path = args.first().ok_or("new: missing <proj.json>")?;
    let name = args.get(1).cloned().unwrap_or_else(|| "Untitled".into());
    let project = Project::new(name, SampleRate::default());
    store(&project, path)?;
    Ok(format!("created project '{}' at {path}", project.name))
}

fn cmd_show(args: &[String]) -> CliResult {
    let path = args.first().ok_or("show: missing <proj.json>")?;
    let p = load(path)?;
    Ok(describe(&p))
}

fn cmd_import(args: &[String]) -> CliResult {
    let path = args.first().ok_or("import: missing <proj.json>")?;
    let wav = args.get(1).ok_or("import: missing <file.wav>")?;
    let mut p = load(path)?;

    // Import at the project rate so clips line up; store only the reference.
    let imported =
        daw_media::import_wav_at(wav, p.sample_rate).map_err(|e| format!("import failed: {e}"))?;
    let asset: MediaAssetRef = imported.asset;
    let id = asset.id;
    let frames = asset.frames.frames();
    let ch = match asset.channels {
        ChannelLayout::Mono => "mono",
        ChannelLayout::Stereo => "stereo",
    };
    p.add_asset(asset);
    store(&p, path)?;
    Ok(format!(
        "imported {wav} as asset {id} ({ch}, {frames} frames @ {} Hz)",
        p.sample_rate.hz()
    ))
}

fn cmd_add_track(args: &[String]) -> CliResult {
    let path = args.first().ok_or("add-track: missing <proj.json>")?;
    let name = args.get(1).ok_or("add-track: missing <name>")?;
    let kind = track_kind(args.get(2).map(String::as_str).unwrap_or("audio"));
    let mut p = load(path)?;
    let id = p.add_track(Track::new(kind, name.clone()));
    p.validate().map_err(|e| e.to_string())?;
    store(&p, path)?;
    Ok(format!("added track '{name}' ({id})"))
}

fn cmd_add_clip(args: &[String]) -> CliResult {
    let path = args.first().ok_or("add-clip: missing <proj.json>")?;
    let track_ref = args.get(1).ok_or("add-clip: missing <track>")?;
    let asset_ref = args.get(2).ok_or("add-clip: missing <asset>")?;
    let mut p = load(path)?;
    let track_id = resolve_track(&p, track_ref)?;
    let asset_id = resolve_asset(&p, asset_ref)?;
    let asset_frames = p
        .asset(asset_id)
        .map(|a| a.frames)
        .unwrap_or(SampleTime::ZERO);

    let start = args
        .get(3)
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "start_frames must be an integer")?
        .map(SampleTime::new)
        .unwrap_or(SampleTime::ZERO);
    let dur = args
        .get(4)
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "dur_frames must be an integer")?
        .map(SampleTime::new)
        .unwrap_or(asset_frames);

    let clip = AudioClip::new(asset_id, start, dur);
    let clip_id = clip.id;
    p.track_mut(track_id)
        .ok_or("track vanished")?
        .add_clip(clip);
    p.validate().map_err(|e| e.to_string())?;
    store(&p, path)?;
    Ok(format!(
        "added clip {clip_id} on track {track_id} (start {}, dur {})",
        start.frames(),
        dur.frames()
    ))
}

fn cmd_add_effect(args: &[String]) -> CliResult {
    let path = args.first().ok_or("add-effect: missing <proj.json>")?;
    let track_ref = args.get(1).ok_or("add-effect: missing <track>")?;
    let ty = args.get(2).ok_or("add-effect: missing <type>")?;
    let mut p = load(path)?;
    let track_id = resolve_track(&p, track_ref)?;

    let effect = apply_params(EffectInstance::new(ty.clone()), &args[3.min(args.len())..])?;
    let effect_id: EffectId = effect.id;
    p.track_mut(track_id)
        .ok_or("track vanished")?
        .effects
        .push(effect);
    p.validate().map_err(|e| e.to_string())?;
    store(&p, path)?;
    Ok(format!("added {ty} effect {effect_id} to track {track_id}"))
}

fn cmd_automate(args: &[String]) -> CliResult {
    let path = args.first().ok_or("automate: missing <proj.json>")?;
    let track_ref = args.get(1).ok_or("automate: missing <track>")?;
    let target_kind = args.get(2).ok_or("automate: missing target (volume)")?;
    let mut p = load(path)?;
    let track_id = resolve_track(&p, track_ref)?;

    let target = match target_kind.as_str() {
        "volume" => AutomationTarget::TrackVolume(track_id),
        "pan" => AutomationTarget::TrackPan(track_id),
        // "effect:<effect-id-or-type>:<param>" — automate an effect parameter.
        s if s.starts_with("effect:") => {
            let spec = &s["effect:".len()..];
            let (effect_ref, param) = spec.split_once(':').ok_or(
                "effect target must be effect:<id-or-type>:<param> (e.g. effect:filter:cutoff)",
            )?;
            let effect_id = resolve_effect(&p, track_id, effect_ref)?;
            AutomationTarget::EffectParam {
                track: track_id,
                effect: effect_id,
                param: param.to_string(),
            }
        }
        other => {
            return Err(format!(
                "unsupported automation target '{other}' (use volume, pan, or effect:<id-or-type>:<param>)"
            ))
        }
    };

    let mut lane = AutomationLane::new(target, Interpolation::Linear);
    let mut count = 0;
    for spec in &args[3.min(args.len())..] {
        let (t, v) = spec
            .split_once(':')
            .ok_or_else(|| format!("point '{spec}' must be time:value"))?;
        let time = t.parse::<u64>().map_err(|_| "point time must be frames")?;
        let value = v
            .parse::<f32>()
            .map_err(|_| "point value must be a number")?;
        lane.add_point(AutomationPoint::new(SampleTime::new(time), value));
        count += 1;
    }
    if count == 0 {
        return Err("automate: provide at least one time:value point".into());
    }
    // Replace any existing lane for this target.
    if let Some(idx) = p.automation_index(&lane.target) {
        p.automation[idx] = lane;
    } else {
        p.automation.push(lane);
    }
    p.validate().map_err(|e| e.to_string())?;
    store(&p, path)?;
    Ok(format!(
        "set {count}-point volume automation on track {track_id}"
    ))
}

fn cmd_set_volume(args: &[String]) -> CliResult {
    let path = args.first().ok_or("set-volume: missing <proj.json>")?;
    let track_ref = args.get(1).ok_or("set-volume: missing <track>")?;
    let gain = args
        .get(2)
        .ok_or("set-volume: missing <linear_gain>")?
        .parse::<f32>()
        .map_err(|_| "gain must be a number")?;
    let mut p = load(path)?;
    let track_id = resolve_track(&p, track_ref)?;
    let g = Gain::linear(gain).ok_or("gain must be finite and >= 0")?;
    p.track_mut(track_id).ok_or("track vanished")?.volume = g;
    store(&p, path)?;
    Ok(format!("set track {track_id} volume to {gain}"))
}

fn cmd_export(args: &[String]) -> CliResult {
    let path = args.first().ok_or("export: missing <proj.json>")?;
    let out = args.get(1).ok_or("export: missing <out.wav>")?;
    let format = match args.get(2).map(String::as_str) {
        Some("float32") => WavFormat::Float32,
        _ => WavFormat::Pcm16,
    };
    let p = load(path)?;
    let source = source_for(&p)?;
    daw_render::export_wav(
        &p,
        &source,
        out,
        &ExportOptions {
            format,
            ..Default::default()
        },
    )
    .map_err(|e| format!("export failed: {e}"))?;

    let secs = daw_engine::project_length_frames(&p.tracks).frames() as f64
        / f64::from(p.sample_rate.hz());
    Ok(format!(
        "exported {out} ({secs:.3}s, {})",
        if matches!(format, WavFormat::Float32) {
            "float32"
        } else {
            "pcm16"
        }
    ))
}

// --- human-readable project summary ---------------------------------------

/// Renders a detailed, human-readable summary of a project.
#[must_use]
pub fn describe(p: &Project) -> String {
    let mut out = String::new();
    out.push_str(&format!("Project: {}\n", p.name));
    out.push_str(&format!("  sample rate: {} Hz\n", p.sample_rate.hz()));
    out.push_str(&format!("  tempo:       {} bpm\n", p.tempo.bpm()));
    out.push_str(&format!("  assets:      {}\n", p.assets.len()));
    for a in &p.assets {
        out.push_str(&format!(
            "    - {} ({} frames) [{}]\n",
            a.path,
            a.frames.frames(),
            a.id
        ));
    }
    out.push_str(&format!("  tracks:      {}\n", p.tracks.len()));
    for (i, t) in p.tracks.iter().enumerate() {
        let kind = match t.kind {
            TrackKind::Audio => "audio",
            TrackKind::Midi => "midi",
            TrackKind::Bus => "bus",
            TrackKind::Master => "master",
        };
        out.push_str(&format!(
            "    [{i}] {kind:<6} \"{}\" vol={:.2}{}{} — {} clip(s), {} effect(s)\n",
            t.name,
            t.volume.factor(),
            if t.mute { " MUTE" } else { "" },
            if t.solo { " SOLO" } else { "" },
            t.clips.len(),
            t.effects.len(),
        ));
        for e in &t.effects.effects {
            out.push_str(&format!(
                "         fx: {}{}\n",
                e.processor_type,
                if e.bypassed { " (bypassed)" } else { "" }
            ));
        }
    }
    out.push_str(&format!("  automation:  {} lane(s)\n", p.automation.len()));
    for lane in &p.automation {
        out.push_str(&format!(
            "    - {:?}: {} point(s)\n",
            lane.target,
            lane.points().len()
        ));
    }
    out
}

/// Returns `true` if `path` looks like an existing project file (for callers).
#[must_use]
pub fn is_project_file(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    #[test]
    fn new_show_add_track_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.json");
        let proj = proj.to_str().unwrap();

        run(&s(&["new", proj, "Song"])).unwrap();
        run(&s(&["add-track", proj, "Gtr", "audio"])).unwrap();
        let shown = run(&s(&["show", proj])).unwrap();
        assert!(shown.contains("Project: Song"));
        assert!(shown.contains("\"Gtr\""));
    }

    #[test]
    fn add_effect_with_params() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.json");
        let proj = proj.to_str().unwrap();
        run(&s(&["new", proj])).unwrap();
        run(&s(&["add-track", proj, "Gtr"])).unwrap();
        run(&s(&[
            "add-effect",
            proj,
            "Gtr",
            "distortion",
            "drive=5",
            "output=0.8",
        ]))
        .unwrap();
        let shown = run(&s(&["show", proj])).unwrap();
        assert!(shown.contains("fx: distortion"));
    }

    #[test]
    fn automate_sets_a_lane() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.json");
        let proj = proj.to_str().unwrap();
        run(&s(&["new", proj])).unwrap();
        run(&s(&["add-track", proj, "Gtr"])).unwrap();
        run(&s(&["automate", proj, "Gtr", "volume", "0:0", "48000:1"])).unwrap();
        let shown = run(&s(&["show", proj])).unwrap();
        assert!(shown.contains("automation:  1 lane(s)"));
    }

    #[test]
    fn automate_effect_param_is_a_wah() {
        // Add a filter, then automate its cutoff — the sweep is a wah.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.json");
        let proj = proj.to_str().unwrap();
        run(&s(&["new", proj])).unwrap();
        run(&s(&["add-track", proj, "Gtr"])).unwrap();
        run(&s(&[
            "add-effect",
            proj,
            "Gtr",
            "filter",
            "mode=2",
            "resonance=0.7",
        ]))
        .unwrap();
        run(&s(&[
            "automate",
            proj,
            "Gtr",
            "effect:filter:cutoff",
            "0:300",
            "24000:3000",
            "48000:300",
        ]))
        .unwrap();
        let shown = run(&s(&["show", proj])).unwrap();
        assert!(shown.contains("automation:  1 lane(s)"));
        assert!(shown.contains("EffectParam"));
    }

    #[test]
    fn unknown_command_errors() {
        assert!(run(&s(&["frobnicate"])).is_err());
    }

    #[test]
    fn help_is_available() {
        assert!(run(&s(&["help"])).unwrap().contains("Usage"));
    }

    #[test]
    fn add_clip_requires_existing_track_and_asset() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("p.json");
        let proj = proj.to_str().unwrap();
        run(&s(&["new", proj])).unwrap();
        // No such track/asset yet.
        assert!(run(&s(&["add-clip", proj, "Nope", "nope"])).is_err());
    }
}
