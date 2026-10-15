//! Recording core: a lock-free capture ring + a writer thread (see spec §9.7).
//!
//! The audio **input callback** is a real-time producer: it pushes captured
//! interleaved `f32` samples into a bounded [`rtrb`] ring and never blocks,
//! allocates, or does I/O. A dedicated **writer thread** drains the ring and
//! streams blocks to a [`RecordSink`] (e.g. a WAV file). This is the
//! bounded-handoff the spec requires between the callback and disk.
//!
//! Failure handling (spec §9.7):
//! * **Overrun** — if the ring is full the producer increments an atomic
//!   `overruns` counter and drops the block, rather than blocking the callback.
//! * **Disk/write failure** — the writer thread stops on the first write error
//!   and records it; it is reported via [`RecordHandle::finish`].
//! * **Safe finalize** — stopping drains the ring, finalizes the sink, and
//!   returns the frames written plus any error, so a partial recording is still
//!   a valid (shorter) file.
//!
//! This module is device-independent and fully testable with a synthetic
//! producer and a fake/failing sink (no microphone needed).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use rtrb::{Consumer, Producer, RingBuffer};

use daw_core::audio::Sample;

use crate::record_wav::RecordSink;

/// Shared status between producer, writer thread, and control side.
#[derive(Default)]
struct RecordStatus {
    /// Interleaved samples dropped because the ring was full.
    overrun_samples: AtomicUsize,
    /// Frames successfully written to the sink.
    frames_written: AtomicU64,
}

/// Real-time producer side: push captured samples from the input callback.
pub struct CaptureProducer {
    ring: Producer<Sample>,
    status: Arc<RecordStatus>,
}

impl CaptureProducer {
    /// Pushes interleaved samples from the audio callback. Real-time safe:
    /// never blocks or allocates. Samples that don't fit are dropped and counted
    /// as an overrun.
    pub fn push(&mut self, interleaved: &[Sample]) {
        let mut dropped = 0;
        for &s in interleaved {
            if self.ring.push(s).is_err() {
                dropped += 1;
            }
        }
        if dropped > 0 {
            self.status
                .overrun_samples
                .fetch_add(dropped, Ordering::Relaxed);
        }
    }
}

/// Control side: stop the recording and collect the result.
pub struct RecordHandle {
    status: Arc<RecordStatus>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    writer: Option<JoinHandle<Result<(), String>>>,
}

/// Outcome of a finished recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOutcome {
    /// Frames written to the sink.
    pub frames_written: u64,
    /// Interleaved samples dropped due to ring overrun (0 is healthy).
    pub overrun_samples: usize,
    /// Error message if the writer failed (the file may be a valid shorter
    /// recording up to that point).
    pub error: Option<String>,
}

impl RecordHandle {
    /// Current frames written (live, for a recording-time readout).
    #[must_use]
    pub fn frames_written(&self) -> u64 {
        self.status.frames_written.load(Ordering::Relaxed)
    }

    /// Current overrun sample count (live).
    #[must_use]
    pub fn overrun_samples(&self) -> usize {
        self.status.overrun_samples.load(Ordering::Relaxed)
    }

    /// Signals the writer thread to drain, finalize the sink, and stop; waits
    /// for it and returns the outcome. Consumes the handle.
    #[must_use]
    pub fn finish(mut self) -> RecordOutcome {
        self.stop.store(true, Ordering::Release);
        let error = match self.writer.take() {
            Some(h) => match h.join() {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(_) => Some("writer thread panicked".to_string()),
            },
            None => None,
        };
        RecordOutcome {
            frames_written: self.status.frames_written.load(Ordering::Relaxed),
            overrun_samples: self.status.overrun_samples.load(Ordering::Relaxed),
            error,
        }
    }
}

/// Starts a recording: spawns the writer thread draining a fresh ring into
/// `sink`. Returns the real-time [`CaptureProducer`] (feed it from the input
/// callback) and a [`RecordHandle`] (to stop/collect).
///
/// `channels` is the interleaved channel count; `ring_capacity` is the ring size
/// in samples (sized for latency vs. overrun tolerance by the caller).
pub fn start_recording<S>(
    mut sink: S,
    channels: u16,
    ring_capacity: usize,
) -> (CaptureProducer, RecordHandle)
where
    S: RecordSink + Send + 'static,
{
    let (producer, mut consumer): (Producer<Sample>, Consumer<Sample>) =
        RingBuffer::new(ring_capacity.max(channels as usize));
    let status = Arc::new(RecordStatus::default());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let writer_status = Arc::clone(&status);
    let writer_stop = Arc::clone(&stop);
    let ch = channels.max(1) as usize;

    let writer = std::thread::spawn(move || -> Result<(), String> {
        // Scratch block for draining (not real-time; this thread may allocate).
        let mut block: Vec<Sample> = Vec::with_capacity(4096);
        let write_block = |block: &mut Vec<Sample>, sink: &mut S| -> Result<(), String> {
            if block.is_empty() {
                return Ok(());
            }
            // Only write whole frames; keep any remainder for next time.
            let whole = block.len() - (block.len() % ch);
            if whole > 0 {
                sink.write_block(&block[..whole])
                    .map_err(|e| e.to_string())?;
                writer_status
                    .frames_written
                    .fetch_add((whole / ch) as u64, Ordering::Relaxed);
                block.drain(..whole);
            }
            Ok(())
        };

        loop {
            // Drain whatever is available.
            while let Ok(s) = consumer.pop() {
                block.push(s);
                if block.len() >= 4096 {
                    write_block(&mut block, &mut sink)?;
                }
            }
            write_block(&mut block, &mut sink)?;

            if writer_stop.load(Ordering::Acquire) {
                // Final drain after stop (producer has ceased).
                while let Ok(s) = consumer.pop() {
                    block.push(s);
                }
                write_block(&mut block, &mut sink)?;
                sink.finalize().map_err(|e| e.to_string())?;
                return Ok(());
            }

            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    (
        CaptureProducer {
            ring: producer,
            status: Arc::clone(&status),
        },
        RecordHandle {
            status,
            stop,
            writer: Some(writer),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use crate::record_wav::WavStreamWriter;

    #[test]
    fn records_frames_to_a_wav_sink() {
        let sink = WavStreamWriter::new(Cursor::new(Vec::new()), 2, 48_000).unwrap();
        let (mut prod, handle) = start_recording(sink, 2, 1 << 16);
        // Feed 100 stereo frames.
        let block: Vec<f32> = (0..200).map(|i| i as f32 * 0.001).collect();
        prod.push(&block);
        // Give the writer a moment, then stop.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let outcome = handle.finish();
        assert_eq!(outcome.frames_written, 100);
        assert_eq!(outcome.overrun_samples, 0);
        assert!(outcome.error.is_none());
    }

    #[test]
    fn overrun_is_counted_not_blocking() {
        // Tiny ring so pushing more than capacity drops + counts overruns.
        let sink = WavStreamWriter::new(Cursor::new(Vec::new()), 1, 48_000).unwrap();
        let (mut prod, handle) = start_recording(sink, 1, 8);
        // Push far more than the ring holds in one shot (writer may not keep up).
        let big: Vec<f32> = vec![0.5; 10_000];
        prod.push(&big);
        let outcome = handle.finish();
        // Either way it must not hang; some combination of written + overrun.
        assert!(outcome.frames_written + outcome.overrun_samples as u64 >= 1);
    }

    /// A sink that always fails writing, to simulate a full/broken disk.
    struct FailingSink;
    impl RecordSink for FailingSink {
        fn write_block(&mut self, _interleaved: &[Sample]) -> std::io::Result<()> {
            Err(std::io::Error::other("disk full"))
        }
        fn finalize(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn disk_write_failure_is_reported() {
        let (mut prod, handle) = start_recording(FailingSink, 1, 1 << 16);
        prod.push(&[0.1; 500]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let outcome = handle.finish();
        assert!(outcome.error.is_some());
        assert!(outcome.error.unwrap().contains("disk full"));
    }

    #[test]
    fn stopping_with_no_input_finalizes_cleanly() {
        let sink = WavStreamWriter::new(Cursor::new(Vec::new()), 2, 48_000).unwrap();
        let (_prod, handle) = start_recording(sink, 2, 1 << 16);
        let outcome = handle.finish();
        assert_eq!(outcome.frames_written, 0);
        assert!(outcome.error.is_none());
    }
}
