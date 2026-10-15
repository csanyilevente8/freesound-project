//! Effect instance model and insert chains (see spec §5.6, §10.5).
//!
//! This is the **persistent, UI- and DSP-independent** description of an effect:
//! a stable id, a processor *type* identifier (a string key, so the project
//! file never embeds Rust types), enabled/bypass state, and a versioned bag of
//! parameter values. It deliberately does **not** hold a live DSP processor —
//! instantiating a processor from a descriptor is an engine/factory concern, so
//! the core stays free of audio code (spec §4.3).
//!
//! Unknown processor type ids survive load as a recoverable "missing effect"
//! rather than corrupting the project (spec §10.5): the descriptor is kept
//! intact; the engine decides how to represent it (e.g. bypassed placeholder).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::EffectId;

/// A parameter value stored in a descriptor. Kept to a small, serialization-
/// friendly set; normalized/real mapping is the processor's concern.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValue {
    /// A floating-point value.
    Float(f32),
    /// An integer value (e.g. an enum/mode selection).
    Int(i64),
    /// A boolean flag.
    Bool(bool),
}

impl ParamValue {
    /// Returns the value as `f32` if it is a float.
    #[must_use]
    pub fn as_float(self) -> Option<f32> {
        match self {
            ParamValue::Float(v) => Some(v),
            _ => None,
        }
    }
}

/// A persistent effect instance descriptor.
///
/// Parameters are a `BTreeMap` keyed by stable parameter-name strings so the
/// serialized form is stable and order-independent (and diff-friendly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    /// Stable instance id.
    pub id: EffectId,
    /// Processor type identifier (a registry key, e.g. "gain", "distortion").
    pub processor_type: String,
    /// Whether the effect is bypassed (passes audio through unprocessed).
    #[serde(default)]
    pub bypassed: bool,
    /// Schema version of this effect's parameter set, for per-effect migration.
    #[serde(default = "default_param_version")]
    pub param_version: u32,
    /// Parameter values keyed by stable parameter name.
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
}

fn default_param_version() -> u32 {
    1
}

impl EffectInstance {
    /// Creates an effect instance of the given processor type with no params.
    #[must_use]
    pub fn new(processor_type: impl Into<String>) -> Self {
        Self {
            id: EffectId::new(),
            processor_type: processor_type.into(),
            bypassed: false,
            param_version: 1,
            params: BTreeMap::new(),
        }
    }

    /// Builder: sets a parameter.
    #[must_use]
    pub fn with_param(mut self, name: impl Into<String>, value: ParamValue) -> Self {
        self.params.insert(name.into(), value);
        self
    }

    /// Gets a parameter value by name.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<ParamValue> {
        self.params.get(name).copied()
    }

    /// Sets a parameter value, returning the previous value if any.
    pub fn set_param(&mut self, name: impl Into<String>, value: ParamValue) -> Option<ParamValue> {
        self.params.insert(name.into(), value)
    }
}

/// An ordered chain of insert effects on a track.
///
/// Order is significant: effects process in list order. Reordering, enabling,
/// and bypassing are plain list/field operations so they are trivially covered
/// by the command/undo layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectChain {
    /// The ordered effects.
    pub effects: Vec<EffectInstance>,
}

impl EffectChain {
    /// Creates an empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of effects in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether the chain is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Appends an effect, returning its id.
    pub fn push(&mut self, effect: EffectInstance) -> EffectId {
        let id = effect.id;
        self.effects.push(effect);
        id
    }

    /// Returns the index of an effect by id.
    #[must_use]
    pub fn index_of(&self, id: EffectId) -> Option<usize> {
        self.effects.iter().position(|e| e.id == id)
    }

    /// Gets an effect by id.
    #[must_use]
    pub fn get(&self, id: EffectId) -> Option<&EffectInstance> {
        self.effects.iter().find(|e| e.id == id)
    }

    /// Gets an effect mutably by id.
    #[must_use]
    pub fn get_mut(&mut self, id: EffectId) -> Option<&mut EffectInstance> {
        self.effects.iter_mut().find(|e| e.id == id)
    }

    /// Removes an effect by id, returning `(index, effect)` if present.
    pub fn remove(&mut self, id: EffectId) -> Option<(usize, EffectInstance)> {
        let idx = self.index_of(id)?;
        Some((idx, self.effects.remove(idx)))
    }

    /// Inserts an effect at `index` (clamped).
    pub fn insert(&mut self, index: usize, effect: EffectInstance) {
        let index = index.min(self.effects.len());
        self.effects.insert(index, effect);
    }

    /// Moves the effect at `from` to `to` (both clamped). No-op if equal.
    pub fn move_effect(&mut self, from: usize, to: usize) {
        if from >= self.effects.len() {
            return;
        }
        let to = to.min(self.effects.len() - 1);
        if from == to {
            return;
        }
        let e = self.effects.remove(from);
        self.effects.insert(to, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_builder_and_access() {
        let e = EffectInstance::new("distortion")
            .with_param("drive", ParamValue::Float(4.0))
            .with_param("output", ParamValue::Float(0.8));
        assert_eq!(e.param("drive"), Some(ParamValue::Float(4.0)));
        assert_eq!(e.param("missing"), None);
        assert!(!e.bypassed);
    }

    #[test]
    fn chain_push_get_remove() {
        let mut c = EffectChain::new();
        let id = c.push(EffectInstance::new("gain"));
        assert_eq!(c.len(), 1);
        assert!(c.get(id).is_some());
        let (idx, e) = c.remove(id).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(e.processor_type, "gain");
        assert!(c.is_empty());
    }

    #[test]
    fn chain_reorder() {
        let mut c = EffectChain::new();
        c.push(EffectInstance::new("a"));
        c.push(EffectInstance::new("b"));
        c.push(EffectInstance::new("c"));
        c.move_effect(0, 2); // a to end -> b, c, a
        assert_eq!(c.effects[0].processor_type, "b");
        assert_eq!(c.effects[1].processor_type, "c");
        assert_eq!(c.effects[2].processor_type, "a");
    }

    #[test]
    fn move_out_of_range_is_noop() {
        let mut c = EffectChain::new();
        c.push(EffectInstance::new("a"));
        c.move_effect(5, 0);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn serde_round_trip() {
        let mut c = EffectChain::new();
        c.push(
            EffectInstance::new("filter")
                .with_param("cutoff", ParamValue::Float(1200.0))
                .with_param("mode", ParamValue::Int(1)),
        );
        let json = serde_json::to_string(&c).unwrap();
        let back: EffectChain = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn unknown_processor_type_survives_round_trip() {
        // A descriptor with an unknown type must deserialize intact (recoverable
        // missing-effect state, spec §10.5).
        let json = r#"{
            "effects": [
                { "id": "33333333-3333-4333-8333-333333333333",
                  "processor_type": "some_future_reverb",
                  "bypassed": false, "param_version": 1,
                  "params": { "size": { "float": 0.7 } } }
            ]
        }"#;
        let c: EffectChain = serde_json::from_str(json).unwrap();
        assert_eq!(c.effects[0].processor_type, "some_future_reverb");
        assert_eq!(c.effects[0].param("size"), Some(ParamValue::Float(0.7)));
    }
}
