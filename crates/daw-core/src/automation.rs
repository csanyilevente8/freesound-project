//! Automation model (see spec §5.5).
//!
//! An [`AutomationLane`] targets a stable parameter (an [`AutomationTarget`])
//! and stores an ordered list of time/value [`AutomationPoint`]s plus an
//! [`Interpolation`] mode. Automation is a **general** feature: there is no
//! per-effect special case (spec §8.6) — any automatable target is addressed by
//! the same lane type.
//!
//! Targets are expressed in terms of **stable ids** (track id, effect id +
//! parameter name), never widget positions or array indices (spec §5.5, §5.6).
//! Values are normalized `f32` in the lane; mapping a normalized value to a DSP
//! value is the processor's concern.
//!
//! Evaluation is deterministic and offline-friendly: [`AutomationLane::value_at`]
//! returns the interpolated value at any sample position, with step/hold or
//! linear interpolation and flat extrapolation beyond the first/last point.

use serde::{Deserialize, Serialize};

use crate::id::{EffectId, TrackId};
use crate::time::SampleTime;
use crate::validate::{Validate, ValidationError};

/// What a lane automates. Addressed by stable ids only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTarget {
    /// A track's output volume.
    TrackVolume(TrackId),
    /// A track's pan position.
    TrackPan(TrackId),
    /// A named parameter of an effect instance on a track.
    EffectParam {
        /// Track owning the effect.
        track: TrackId,
        /// Effect instance id.
        effect: EffectId,
        /// Stable parameter name within the effect.
        param: String,
    },
}

/// How to interpolate between automation points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Hold the previous point's value until the next point (step).
    Step,
    /// Linearly interpolate between adjacent points.
    Linear,
}

/// A single automation point: a value at a timeline position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Timeline position (sample frames at project rate).
    pub time: SampleTime,
    /// Normalized value, typically in [0, 1].
    pub value: f32,
}

impl AutomationPoint {
    /// Creates a point.
    #[must_use]
    pub fn new(time: SampleTime, value: f32) -> Self {
        Self { time, value }
    }
}

/// An automation lane: a target, an interpolation mode, and sorted points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    /// What this lane controls.
    pub target: AutomationTarget,
    /// Interpolation between points.
    pub interpolation: Interpolation,
    /// Points, kept sorted ascending by time (enforced by [`add_point`] and
    /// checked by [`Validate`]).
    ///
    /// [`add_point`]: AutomationLane::add_point
    points: Vec<AutomationPoint>,
}

impl AutomationLane {
    /// Creates an empty lane for `target` with the given interpolation.
    #[must_use]
    pub fn new(target: AutomationTarget, interpolation: Interpolation) -> Self {
        Self {
            target,
            interpolation,
            points: Vec::new(),
        }
    }

    /// Read-only view of the points (always sorted by time).
    #[must_use]
    pub fn points(&self) -> &[AutomationPoint] {
        &self.points
    }

    /// Inserts a point, keeping the list sorted by time. If a point already
    /// exists at the exact same time, it is replaced.
    pub fn add_point(&mut self, point: AutomationPoint) {
        match self.points.binary_search_by(|p| p.time.cmp(&point.time)) {
            Ok(i) => self.points[i] = point, // replace at identical time
            Err(i) => self.points.insert(i, point),
        }
    }

    /// Removes the point at exactly `time`, returning it if present.
    pub fn remove_point_at(&mut self, time: SampleTime) -> Option<AutomationPoint> {
        match self.points.binary_search_by(|p| p.time.cmp(&time)) {
            Ok(i) => Some(self.points.remove(i)),
            Err(_) => None,
        }
    }

    /// Evaluates the lane at `time`.
    ///
    /// Returns `None` only if the lane has no points. Before the first point and
    /// after the last point the value is held flat (the first/last value). Between
    /// points it uses the lane's interpolation mode.
    #[must_use]
    pub fn value_at(&self, time: SampleTime) -> Option<f32> {
        if self.points.is_empty() {
            return None;
        }
        // Flat extrapolation at the ends.
        let first = self.points[0];
        if time <= first.time {
            return Some(first.value);
        }
        let last = self.points[self.points.len() - 1];
        if time >= last.time {
            return Some(last.value);
        }

        // Find the segment [a, b) containing `time`. partition_point gives the
        // number of points with time <= `time`; since first.time < time < last.time,
        // this is in 1..len.
        let idx = self.points.partition_point(|p| p.time <= time);
        let a = self.points[idx - 1];
        let b = self.points[idx];

        match self.interpolation {
            Interpolation::Step => Some(a.value),
            Interpolation::Linear => {
                let span = (b.time.frames() - a.time.frames()) as f32;
                if span <= 0.0 {
                    return Some(b.value);
                }
                let t = (time.frames() - a.time.frames()) as f32 / span;
                Some(a.value + (b.value - a.value) * t)
            }
        }
    }
}

impl Validate for AutomationLane {
    fn validate(&self) -> Result<(), ValidationError> {
        // Points must be strictly increasing in time and have finite values.
        let mut prev: Option<SampleTime> = None;
        for p in &self.points {
            if !p.value.is_finite() {
                return Err(ValidationError::new("automation point value not finite"));
            }
            if let Some(prev_t) = prev {
                if p.time <= prev_t {
                    return Err(ValidationError::new(
                        "automation points must be strictly increasing in time",
                    ));
                }
            }
            prev = Some(p.time);
        }
        if let AutomationTarget::EffectParam { param, .. } = &self.target {
            if param.trim().is_empty() {
                return Err(ValidationError::new(
                    "automation effect-param name must not be empty",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(interp: Interpolation) -> AutomationLane {
        AutomationLane::new(AutomationTarget::TrackVolume(TrackId::new()), interp)
    }

    #[test]
    fn empty_lane_has_no_value() {
        assert_eq!(lane(Interpolation::Linear).value_at(SampleTime::ZERO), None);
    }

    #[test]
    fn add_point_keeps_sorted_and_replaces_at_same_time() {
        let mut l = lane(Interpolation::Linear);
        l.add_point(AutomationPoint::new(SampleTime::new(100), 0.5));
        l.add_point(AutomationPoint::new(SampleTime::new(0), 0.0));
        l.add_point(AutomationPoint::new(SampleTime::new(50), 0.25));
        let times: Vec<u64> = l.points().iter().map(|p| p.time.frames()).collect();
        assert_eq!(times, vec![0, 50, 100]);
        // Replace at same time.
        l.add_point(AutomationPoint::new(SampleTime::new(50), 0.9));
        assert_eq!(l.points().len(), 3);
        assert!((l.value_at(SampleTime::new(50)).unwrap() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn flat_extrapolation_at_ends() {
        let mut l = lane(Interpolation::Linear);
        l.add_point(AutomationPoint::new(SampleTime::new(100), 0.2));
        l.add_point(AutomationPoint::new(SampleTime::new(200), 0.8));
        assert_eq!(l.value_at(SampleTime::new(0)), Some(0.2)); // before first
        assert_eq!(l.value_at(SampleTime::new(500)), Some(0.8)); // after last
    }

    #[test]
    fn linear_interpolation_midpoint() {
        let mut l = lane(Interpolation::Linear);
        l.add_point(AutomationPoint::new(SampleTime::new(0), 0.0));
        l.add_point(AutomationPoint::new(SampleTime::new(100), 1.0));
        assert!((l.value_at(SampleTime::new(50)).unwrap() - 0.5).abs() < 1e-6);
        assert!((l.value_at(SampleTime::new(25)).unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn step_holds_previous_value() {
        let mut l = lane(Interpolation::Step);
        l.add_point(AutomationPoint::new(SampleTime::new(0), 0.0));
        l.add_point(AutomationPoint::new(SampleTime::new(100), 1.0));
        assert_eq!(l.value_at(SampleTime::new(99)), Some(0.0));
        assert_eq!(l.value_at(SampleTime::new(100)), Some(1.0));
    }

    #[test]
    fn remove_point() {
        let mut l = lane(Interpolation::Step);
        l.add_point(AutomationPoint::new(SampleTime::new(10), 0.3));
        assert!(l.remove_point_at(SampleTime::new(10)).is_some());
        assert!(l.remove_point_at(SampleTime::new(10)).is_none());
        assert!(l.points().is_empty());
    }

    #[test]
    fn validate_rejects_non_finite_and_unsorted() {
        // Construct directly to bypass add_point sorting, to test validation.
        let mut l = lane(Interpolation::Linear);
        l.points = vec![
            AutomationPoint::new(SampleTime::new(10), 0.0),
            AutomationPoint::new(SampleTime::new(5), 0.0), // out of order
        ];
        assert!(l.validate().is_err());

        let mut l2 = lane(Interpolation::Linear);
        l2.points = vec![AutomationPoint::new(SampleTime::new(0), f32::NAN)];
        assert!(l2.validate().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let mut l = AutomationLane::new(
            AutomationTarget::EffectParam {
                track: TrackId::new(),
                effect: EffectId::new(),
                param: "cutoff".into(),
            },
            Interpolation::Linear,
        );
        l.add_point(AutomationPoint::new(SampleTime::new(0), 0.1));
        l.add_point(AutomationPoint::new(SampleTime::new(480), 0.9));
        let json = serde_json::to_string(&l).unwrap();
        let back: AutomationLane = serde_json::from_str(&json).unwrap();
        assert_eq!(l, back);
    }
}
