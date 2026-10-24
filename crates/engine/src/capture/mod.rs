//! Audio capture seam. `AudioCaptureProvider` hides the platform API; the engine only sees
//! `CaptureMsg`s on a bounded channel.
//!
//! Implementations:
//! - `cpal_provider::CpalProvider` — Windows WASAPI via cpal: microphone input streams, and
//!   system audio by opening an *input* stream on a render (output) device, which cpal maps to
//!   WASAPI loopback. macOS (ScreenCaptureKit / Core Audio taps) and Linux (PipeWire monitor
//!   sources) are future providers behind the same trait.
//! - `fake::FakeProvider` — deterministic, device-free provider for tests: plays given sample
//!   buffers in real time (or as fast as possible) then idles with silence.
//!
//! Threading contract: a provider's audio callback must never block or allocate unboundedly.
//! It copies the device buffer into a `CaptureMsg::Audio` and `try_send`s it; if the channel
//! is full the buffer is dropped and counted (`CaptureStream::dropped_buffers`), and the engine
//! surfaces a notice. Resampling, VAD and encoding happen on engine threads, never in the
//! callback.

pub mod cpal_provider;
pub mod fake;

use std::time::Instant;

use crossbeam_channel::Sender;

use crate::types::{DeviceInfo, Source};
use crate::Result;

/// Buffers queued between a device callback and the engine. At typical 10 ms WASAPI periods
/// this is ~5 s of audio per source.
pub const CAPTURE_CHANNEL_CAPACITY: usize = 512;

#[derive(Debug, Clone)]
pub enum CaptureMsg {
    /// Interleaved f32 samples in the device's native format (see `Opened`).
    Audio { source: Source, data: Vec<f32>, at: Instant },
    /// Stream is live. Sent before any `Audio` for this stream.
    Opened { source: Source, device_id: String, device_name: String, sample_rate: u32, channels: u16 },
    /// The stream failed (device unplugged, format change, driver error). The stream is dead;
    /// the engine decides whether to reopen.
    Failed { source: Source, error: String },
}

pub trait CaptureStream: Send {
    fn source(&self) -> Source;
    fn dropped_buffers(&self) -> u64;
    /// Stop and release the device. Must be idempotent; `Drop` calls it too.
    fn stop(&mut self);
}

pub trait AudioCaptureProvider: Send + Sync {
    /// Inputs (for `Source::Mic`) and loopback-capable outputs (for `Source::System`).
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    /// `device_id: None` = current system default for that kind.
    fn open(&self, source: Source, device_id: Option<&str>, tx: Sender<CaptureMsg>) -> Result<Box<dyn CaptureStream>>;
    /// Id of the current default device for the source's kind, used to follow default-device
    /// changes when the user hasn't pinned a device.
    fn default_device_id(&self, source: Source) -> Option<String>;
}
