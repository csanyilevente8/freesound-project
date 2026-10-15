//! Applying automation during offline rendering (see spec §5.5, §8.6).
//!
//! Automation is sampled **per block** at the block's start position: this is
//! deterministic, cheap, and adequate for offline export (per-sample ramping of
//! automated values is a later refinement handled by parameter smoothing in the
//! processors, e.g. [`daw_dsp::GainProcessor`]). The approach is general: any
//! [`AutomationTarget`] is resolved the same way (spec §8.6 forbids per-effect
//! special-casing).
//!
//! [`automated_project`] returns a clone of the project with automated values
//! written into the plain fields the renderer already reads (track volume/pan,
//! effect params), so the render path needs no automation-specific branches.

use daw_core::audio::Gain;
use daw_core::automation::AutomationTarget;
use daw_core::effect::ParamValue;
use daw_core::project::Project;
use daw_core::time::SampleTime;

/// Returns a copy of `project` with all automation lanes evaluated at
/// `block_start` and written into the corresponding fields.
///
/// Lanes with no points, or targeting missing entities, are ignored (the
/// project is assumed validated, but this is defensive). Normalized lane values
/// are mapped to DSP values per target:
///
/// * `TrackVolume` — value is treated as a **linear** gain factor in [0, ∞);
///   clamped to ≥ 0.
/// * `TrackPan` — reserved; applied to a future track pan field (no-op today).
/// * `EffectParam` — the normalized value is written as a `Float` param,
///   overriding the stored value for this block.
#[must_use]
pub fn automated_project(project: &Project, block_start: SampleTime) -> Project {
    if project.automation.is_empty() {
        return project.clone();
    }
    let mut out = project.clone();

    for lane in &project.automation {
        let Some(value) = lane.value_at(block_start) else {
            continue;
        };
        match &lane.target {
            AutomationTarget::TrackVolume(id) => {
                if let Some(track) = out.track_mut(*id) {
                    track.volume = Gain::linear(value.max(0.0)).unwrap_or(Gain::SILENCE);
                }
            }
            AutomationTarget::TrackPan(_id) => {
                // Track pan field not yet modeled; reserved for when it exists.
            }
            AutomationTarget::EffectParam {
                track,
                effect,
                param,
            } => {
                if let Some(t) = out.track_mut(*track) {
                    if let Some(e) = t.effects.get_mut(*effect) {
                        e.set_param(param.clone(), ParamValue::Float(value));
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::automation::{AutomationLane, AutomationPoint, Interpolation};
    use daw_core::effect::EffectInstance;
    use daw_core::time::SampleRate;
    use daw_core::track::{Track, TrackKind};

    fn project() -> Project {
        Project::new("Auto", SampleRate::new(48_000).unwrap())
    }

    #[test]
    fn no_automation_returns_equivalent_project() {
        let p = project();
        let out = automated_project(&p, SampleTime::ZERO);
        assert_eq!(p, out);
    }

    #[test]
    fn track_volume_automation_is_applied() {
        let mut p = project();
        let track_id = p.add_track(Track::new(TrackKind::Audio, "A"));
        let mut lane = AutomationLane::new(
            AutomationTarget::TrackVolume(track_id),
            Interpolation::Linear,
        );
        lane.add_point(AutomationPoint::new(SampleTime::new(0), 0.0));
        lane.add_point(AutomationPoint::new(SampleTime::new(100), 1.0));
        p.automation.push(lane);

        // At frame 50, linear automation gives 0.5.
        let out = automated_project(&p, SampleTime::new(50));
        assert!((out.track(track_id).unwrap().volume.factor() - 0.5).abs() < 1e-6);
        // At the start, 0.0.
        let out0 = automated_project(&p, SampleTime::ZERO);
        assert_eq!(out0.track(track_id).unwrap().volume.factor(), 0.0);
    }

    #[test]
    fn effect_param_automation_overrides_stored_value() {
        let mut p = project();
        let mut track = Track::new(TrackKind::Audio, "A");
        let effect = EffectInstance::new("filter").with_param("cutoff", ParamValue::Float(500.0));
        let effect_id = effect.id;
        track.effects.push(effect);
        let track_id = p.add_track(track);

        let mut lane = AutomationLane::new(
            AutomationTarget::EffectParam {
                track: track_id,
                effect: effect_id,
                param: "cutoff".into(),
            },
            Interpolation::Step,
        );
        lane.add_point(AutomationPoint::new(SampleTime::new(0), 2000.0));
        p.automation.push(lane);

        let out = automated_project(&p, SampleTime::new(10));
        let v = out
            .track(track_id)
            .unwrap()
            .effects
            .get(effect_id)
            .unwrap()
            .param("cutoff");
        assert_eq!(v, Some(ParamValue::Float(2000.0)));
    }
}
