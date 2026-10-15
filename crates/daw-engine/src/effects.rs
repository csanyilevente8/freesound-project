//! Effect factory and chain processing (see spec §5.6, §10.5).
//!
//! Bridges the persistent [`EffectInstance`] descriptors in `daw-core` to live
//! [`AudioProcessor`]s in `daw-dsp`. The core model stays free of DSP types; the
//! engine owns the mapping from a string `processor_type` + parameter bag to a
//! concrete processor.
//!
//! Unknown processor types are **recoverable** (spec §10.5): [`build_processor`]
//! returns `None`, and [`process_chain`] treats a missing/bypassed effect as a
//! pass-through rather than failing. This means loading a project that
//! references an effect this build does not know about still plays (that effect
//! is simply skipped) instead of corrupting playback.

use daw_core::effect::{EffectChain, EffectInstance, ParamValue};
use daw_dsp::buffer::AudioBuffer;
use daw_dsp::compressor::CompressorProcessor;
use daw_dsp::delay::DelayProcessor;
use daw_dsp::distortion::DistortionProcessor;
use daw_dsp::eq::{EqBand, EqBandType};
use daw_dsp::filter::{FilterMode, FilterProcessor};
use daw_dsp::gain::GainProcessor;
use daw_dsp::pan::PanProcessor;
use daw_dsp::processor::{AudioProcessor, ProcessConfig};
use daw_dsp::reverb::ReverbProcessor;

/// Reads a float parameter with a default.
fn pf(e: &EffectInstance, name: &str, default: f32) -> f32 {
    e.param(name)
        .and_then(ParamValue::as_float)
        .unwrap_or(default)
}

/// Builds a live processor from a descriptor, or `None` for an unknown type.
///
/// The returned processor is **not** yet prepared; the caller must call
/// `prepare` with the appropriate [`ProcessConfig`] before processing.
#[must_use]
pub fn build_processor(effect: &EffectInstance) -> Option<Box<dyn AudioProcessor>> {
    match effect.processor_type.as_str() {
        "gain" => {
            let factor = pf(effect, "gain", 1.0);
            let g = daw_core::audio::Gain::linear(factor).unwrap_or(daw_core::audio::Gain::UNITY);
            Some(Box::new(GainProcessor::new(g)))
        }
        "pan" => {
            let pan = pf(effect, "pan", 0.0);
            Some(Box::new(PanProcessor::new(pan)))
        }
        "distortion" => {
            let drive = pf(effect, "drive", 1.0);
            let output = pf(effect, "output", 1.0);
            Some(Box::new(DistortionProcessor::new(drive, output)))
        }
        "filter" => {
            let cutoff = pf(effect, "cutoff", 1000.0);
            let resonance = pf(effect, "resonance", 0.0);
            let mode = match effect.param("mode") {
                Some(ParamValue::Int(1)) => FilterMode::Highpass,
                Some(ParamValue::Int(2)) => FilterMode::Bandpass,
                _ => FilterMode::Lowpass,
            };
            Some(Box::new(FilterProcessor::new(mode, cutoff, resonance)))
        }
        "delay" => {
            let time = pf(effect, "time", 0.25);
            let feedback = pf(effect, "feedback", 0.3);
            let mix = pf(effect, "mix", 0.3);
            // Preallocate up to 2s (or the requested time if larger).
            Some(Box::new(DelayProcessor::new(
                time,
                feedback,
                mix,
                2.0_f32.max(time),
            )))
        }
        "reverb" => {
            let room = pf(effect, "room", 0.5);
            let mix = pf(effect, "mix", 0.3);
            Some(Box::new(ReverbProcessor::new(room, mix)))
        }
        "eq" => {
            let freq = pf(effect, "freq", 1000.0);
            let q = pf(effect, "q", 1.0);
            let gain_db = pf(effect, "gain_db", 0.0);
            let band_type = match effect.param("type") {
                Some(ParamValue::Int(1)) => EqBandType::LowShelf,
                Some(ParamValue::Int(2)) => EqBandType::HighShelf,
                _ => EqBandType::Peaking,
            };
            Some(Box::new(EqBand::new(band_type, freq, q, gain_db)))
        }
        "compressor" => {
            let threshold = pf(effect, "threshold_db", -18.0);
            let ratio = pf(effect, "ratio", 4.0);
            let attack = pf(effect, "attack_ms", 10.0);
            let release = pf(effect, "release_ms", 100.0);
            let makeup = pf(effect, "makeup_db", 0.0);
            Some(Box::new(CompressorProcessor::new(
                threshold, ratio, attack, release, makeup,
            )))
        }
        _ => None,
    }
}

/// A prepared chain of processors ready to process blocks.
///
/// Build this once (off the audio thread) from an [`EffectChain`] and a
/// [`ProcessConfig`]; then call [`ProcessorChain::process`] per block. Bypassed
/// and unknown effects are omitted at build time, so `process` does no
/// per-effect branching.
pub struct ProcessorChain {
    processors: Vec<Box<dyn AudioProcessor>>,
}

impl ProcessorChain {
    /// Builds a processor chain from descriptors, skipping bypassed and unknown
    /// effects, preparing each with `config`.
    #[must_use]
    pub fn build(chain: &EffectChain, config: &ProcessConfig) -> Self {
        let mut processors = Vec::new();
        for effect in &chain.effects {
            if effect.bypassed {
                continue;
            }
            if let Some(mut p) = build_processor(effect) {
                p.prepare(config);
                processors.push(p);
            }
            // Unknown type: skipped (recoverable missing effect).
        }
        Self { processors }
    }

    /// Number of active (non-bypassed, known) processors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.processors.len()
    }

    /// Whether the chain has no active processors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.processors.is_empty()
    }

    /// Processes a block through every active processor in order, in place.
    pub fn process(&mut self, buffer: &mut AudioBuffer) {
        for p in &mut self.processors {
            p.process(buffer);
        }
    }
}

/// Convenience: build a chain and process one buffer in a single call. Prefer
/// [`ProcessorChain::build`] once + reuse for repeated blocks.
pub fn process_chain(chain: &EffectChain, config: &ProcessConfig, buffer: &mut AudioBuffer) {
    ProcessorChain::build(chain, config).process(buffer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::effect::EffectInstance;

    fn config() -> ProcessConfig {
        ProcessConfig {
            sample_rate: daw_core::time::SampleRate::new(48_000).unwrap(),
            max_block_frames: 512,
            channels: 2,
        }
    }

    fn stereo(v: f32) -> AudioBuffer {
        let mut b = AudioBuffer::new(2, 8);
        b.channel_mut(0).fill(v);
        b.channel_mut(1).fill(v);
        b
    }

    #[test]
    fn builds_known_processors() {
        assert!(build_processor(&EffectInstance::new("gain")).is_some());
        assert!(build_processor(&EffectInstance::new("pan")).is_some());
        assert!(build_processor(&EffectInstance::new("distortion")).is_some());
        assert!(build_processor(&EffectInstance::new("filter")).is_some());
        assert!(build_processor(&EffectInstance::new("delay")).is_some());
        assert!(build_processor(&EffectInstance::new("reverb")).is_some());
        assert!(build_processor(&EffectInstance::new("eq")).is_some());
        assert!(build_processor(&EffectInstance::new("compressor")).is_some());
    }

    #[test]
    fn unknown_processor_is_none() {
        assert!(build_processor(&EffectInstance::new("warp_drive")).is_none());
    }

    #[test]
    fn gain_effect_applies() {
        let mut chain = EffectChain::new();
        chain.push(EffectInstance::new("gain").with_param("gain", ParamValue::Float(0.5)));
        let mut b = stereo(1.0);
        process_chain(&chain, &config(), &mut b);
        assert!((b.channel(0)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bypassed_effect_is_skipped() {
        let mut chain = EffectChain::new();
        let mut e = EffectInstance::new("gain").with_param("gain", ParamValue::Float(0.0));
        e.bypassed = true;
        chain.push(e);
        let built = ProcessorChain::build(&chain, &config());
        assert!(built.is_empty());

        let mut b = stereo(1.0);
        process_chain(&chain, &config(), &mut b);
        assert!((b.channel(0)[0] - 1.0).abs() < 1e-6); // unchanged
    }

    #[test]
    fn unknown_effect_is_skipped_not_fatal() {
        let mut chain = EffectChain::new();
        chain.push(EffectInstance::new("nonexistent"));
        chain.push(EffectInstance::new("gain").with_param("gain", ParamValue::Float(0.25)));
        let built = ProcessorChain::build(&chain, &config());
        assert_eq!(built.len(), 1); // only the gain

        let mut b = stereo(1.0);
        process_chain(&chain, &config(), &mut b);
        assert!((b.channel(0)[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn chain_order_is_respected() {
        // gain 2.0 then distortion heavily saturates; swap order changes result.
        let mut chain = EffectChain::new();
        chain.push(EffectInstance::new("gain").with_param("gain", ParamValue::Float(2.0)));
        chain.push(
            EffectInstance::new("distortion")
                .with_param("drive", ParamValue::Float(100.0))
                .with_param("output", ParamValue::Float(1.0)),
        );
        let mut b = stereo(0.5);
        process_chain(&chain, &config(), &mut b);
        // 0.5*2=1.0 -> tanh(100) ≈ 1.0
        assert!((b.channel(0)[0] - 1.0).abs() < 1e-2);
    }
}
