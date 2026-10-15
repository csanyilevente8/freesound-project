//! CPAL output device backend (see ADR-0003).
//!
//! This is the **only** file that references CPAL. It builds a default output
//! stream and drives a [`Player`] from the real-time callback, converting the
//! engine's `f32` samples to the device's sample format at the boundary. The
//! [`AudioOutput`] trait keeps this backend replaceable; nothing outside this
//! module sees CPAL types.
//!
//! The callback is the real-time thread: it only calls
//! [`Player::process_interleaved`] (which is allocation/lock/IO-free) plus the
//! device format conversion. No project/engine state is touched here.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use daw_core::audio::Sample;

use crate::player::Player;
use crate::source::BlockSource;

/// A started audio output stream. Dropping it stops playback.
pub trait AudioOutput {
    /// The device sample rate the stream was opened at.
    fn sample_rate(&self) -> u32;
}

/// Errors from opening/starting the output device.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    /// No default output device is available.
    #[error("no default output device")]
    NoOutputDevice,
    /// No default input device is available.
    #[error("no default input device")]
    NoInputDevice,
    /// Could not query the default output configuration.
    #[error("no default output config: {0}")]
    NoConfig(String),
    /// Building the stream failed.
    #[error("failed to build stream: {0}")]
    Build(String),
    /// Starting the stream failed.
    #[error("failed to start stream: {0}")]
    Play(String),
    /// The device uses an unsupported sample format.
    #[error("unsupported sample format: {0:?}")]
    UnsupportedFormat(SampleFormat),
}

/// A live CPAL output stream holding its `Player` in the callback.
pub struct CpalOutput {
    _stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}

impl AudioOutput for CpalOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl CpalOutput {
    /// The number of output channels the device opened with.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the default output device's sample rate and channel count without
    /// opening a stream (useful to prepare material at the right rate).
    ///
    /// # Errors
    /// Returns [`DeviceError`] if no device/config is available.
    pub fn default_output_params() -> Result<(u32, u16), DeviceError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice)?;
        let config = device
            .default_output_config()
            .map_err(|e| DeviceError::NoConfig(e.to_string()))?;
        Ok((config.sample_rate(), config.channels()))
    }

    /// Opens the default output device and starts streaming from `player`.
    ///
    /// The `player`'s `max_block` must be large enough for the device's buffer;
    /// callers typically build it from [`default_output_params`] +  a generous
    /// max block. The stream keeps running until the returned handle is dropped.
    ///
    /// # Errors
    /// Returns [`DeviceError`] on any device/stream failure.
    pub fn start<S>(mut player: Player<S>) -> Result<Self, DeviceError>
    where
        S: BlockSource + 'static,
    {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice)?;
        let supported = device
            .default_output_config()
            .map_err(|e| DeviceError::NoConfig(e.to_string()))?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let config: StreamConfig = supported.into();

        let err_fn = |e| eprintln!("audio stream error: {e}");
        let nch = channels as usize;

        // Reusable interleaved f32 scratch is NOT needed: we write directly into
        // the device buffer for f32, and convert in place for other formats via
        // a small stack path. For stereo+ devices we fill the first two channels
        // and zero the rest.
        let stream = match sample_format {
            SampleFormat::F32 => device
                .build_output_stream(
                    config,
                    move |data: &mut [f32], _| {
                        let frames = data.len() / nch;
                        if nch == 2 {
                            player.process_interleaved(data, frames);
                        } else {
                            fill_multichannel(&mut player, data, frames, nch);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            SampleFormat::I16 => device
                .build_output_stream(
                    config,
                    move |data: &mut [i16], _| {
                        let frames = data.len() / nch;
                        write_converted(&mut player, data, frames, nch, |s| {
                            (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            SampleFormat::U16 => device
                .build_output_stream(
                    config,
                    move |data: &mut [u16], _| {
                        let frames = data.len() / nch;
                        write_converted(&mut player, data, frames, nch, |s| {
                            let v = (s.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32;
                            v as u16
                        });
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            other => return Err(DeviceError::UnsupportedFormat(other)),
        };

        stream
            .play()
            .map_err(|e| DeviceError::Play(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            sample_rate,
            channels,
        })
    }
}

/// Fills a device buffer with more than 2 channels: stereo into ch 0/1, silence
/// elsewhere. Uses a small fixed stack buffer, processing in chunks so no
/// allocation occurs in the callback.
fn fill_multichannel<S: BlockSource>(
    player: &mut Player<S>,
    data: &mut [f32],
    frames: usize,
    nch: usize,
) {
    const CHUNK: usize = 512;
    let mut stereo = [0.0f32; CHUNK * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(CHUNK);
        player.process_interleaved(&mut stereo[..n * 2], n);
        for i in 0..n {
            let base = (done + i) * nch;
            data[base] = stereo[i * 2];
            if nch > 1 {
                data[base + 1] = stereo[i * 2 + 1];
            }
            for c in 2..nch {
                data[base + c] = 0.0;
            }
        }
        done += n;
    }
}

/// Writes converted (non-f32) output by processing stereo f32 in stack chunks
/// then converting with `conv`. Allocation-free in the callback.
fn write_converted<S, T, F>(
    player: &mut Player<S>,
    data: &mut [T],
    frames: usize,
    nch: usize,
    conv: F,
) where
    S: BlockSource,
    T: Copy,
    F: Fn(Sample) -> T,
{
    const CHUNK: usize = 512;
    let mut stereo = [0.0f32; CHUNK * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(CHUNK);
        player.process_interleaved(&mut stereo[..n * 2], n);
        for i in 0..n {
            let base = (done + i) * nch;
            let l = conv(stereo[i * 2]);
            let r = conv(stereo[i * 2 + 1]);
            data[base] = l;
            if nch > 1 {
                data[base + 1] = r;
            }
            for c in 2..nch {
                data[base + c] = conv(0.0);
            }
        }
        done += n;
    }
}

// ===========================================================================
// Input (capture) backend
// ===========================================================================

use crate::record::CaptureProducer;

/// A live CPAL input stream pushing captured samples into a [`CaptureProducer`].
/// Dropping it stops capture.
pub struct CpalInput {
    _stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}

impl CpalInput {
    /// Sample rate the input stream opened at.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count the input stream opened with.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Default input device's sample rate and channel count, without opening a
    /// stream (so the recorder/WAV can be configured to match).
    ///
    /// # Errors
    /// Returns [`DeviceError`] if no input device/config is available.
    pub fn default_input_params() -> Result<(u32, u16), DeviceError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?;
        let config = device
            .default_input_config()
            .map_err(|e| DeviceError::NoConfig(e.to_string()))?;
        Ok((config.sample_rate(), config.channels()))
    }

    /// Opens the default input device and streams captured interleaved `f32`
    /// samples into `producer`. The stream runs until the returned handle is
    /// dropped.
    ///
    /// The callback is the real-time capture thread: it only converts the
    /// device format to `f32` and calls [`CaptureProducer::push`] (which never
    /// blocks or allocates).
    ///
    /// # Errors
    /// Returns [`DeviceError`] on any device/stream failure.
    pub fn start(mut producer: CaptureProducer) -> Result<Self, DeviceError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|e| DeviceError::NoConfig(e.to_string()))?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let config: StreamConfig = supported.into();

        let err_fn = |e| eprintln!("audio input stream error: {e}");

        let stream = match sample_format {
            SampleFormat::F32 => device
                .build_input_stream(
                    config,
                    move |data: &[f32], _| producer.push(data),
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            SampleFormat::I16 => device
                .build_input_stream(
                    config,
                    move |data: &[i16], _| {
                        const CHUNK: usize = 1024;
                        let mut tmp = [0.0f32; CHUNK];
                        let mut i = 0;
                        while i < data.len() {
                            let n = (data.len() - i).min(CHUNK);
                            for k in 0..n {
                                tmp[k] = f32::from(data[i + k]) / f32::from(i16::MAX);
                            }
                            producer.push(&tmp[..n]);
                            i += n;
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            SampleFormat::U16 => device
                .build_input_stream(
                    config,
                    move |data: &[u16], _| {
                        const CHUNK: usize = 1024;
                        let mut tmp = [0.0f32; CHUNK];
                        let mut i = 0;
                        while i < data.len() {
                            let n = (data.len() - i).min(CHUNK);
                            for k in 0..n {
                                tmp[k] = (f32::from(data[i + k]) / f32::from(u16::MAX)) * 2.0 - 1.0;
                            }
                            producer.push(&tmp[..n]);
                            i += n;
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| DeviceError::Build(e.to_string()))?,
            other => return Err(DeviceError::UnsupportedFormat(other)),
        };

        stream
            .play()
            .map_err(|e| DeviceError::Play(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            sample_rate,
            channels,
        })
    }
}
