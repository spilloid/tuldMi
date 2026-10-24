//! Device-free capture provider for tests and demos of the pipeline (never used by the shipped
//! app for user-facing data).

use std::time::Duration;

use super::*;

#[derive(Debug, Clone)]
pub struct FakeSource {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples to play once; afterwards the stream produces digital silence until
    /// stopped (or fails, see `fail_after`).
    pub samples: Vec<f32>,
    /// Device buffer size in frames (per channel).
    pub buffer_frames: usize,
    /// Pace buffers in real time. `false` = as fast as the channel accepts (blocking send, so no
    /// drops) — compresses wall-clock timestamps, fine for logic tests.
    pub realtime: bool,
    /// After this many buffers, send `Failed` and stop producing (simulates unplugging). Only
    /// the first stream opened for this source fails; reopening yields a healthy stream.
    pub fail_after: Option<usize>,
    pub device_name: String,
}

impl FakeSource {
    pub fn mono_16k(samples: Vec<f32>) -> Self {
        FakeSource {
            sample_rate: 16_000,
            channels: 1,
            samples,
            buffer_frames: 160,
            realtime: false,
            fail_after: None,
            device_name: "Fake device".into(),
        }
    }
}

pub struct FakeProvider {
    // Implementation-defined.
}

impl FakeProvider {
    pub fn new() -> Self {
        todo!()
    }
    pub fn with_source(self, source: Source, fake: FakeSource) -> Self {
        let _ = (source, fake);
        todo!()
    }
    /// How many times `open` has been called for `source`.
    pub fn open_count(&self, source: Source) -> usize {
        let _ = source;
        todo!()
    }
    /// Idle interval between silence buffers once `samples` is exhausted (non-realtime mode).
    pub const IDLE_TICK: Duration = Duration::from_millis(10);
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCaptureProvider for FakeProvider {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        todo!()
    }
    fn open(&self, source: Source, device_id: Option<&str>, tx: Sender<CaptureMsg>) -> Result<Box<dyn CaptureStream>> {
        let _ = (source, device_id, tx);
        todo!()
    }
    fn default_device_id(&self, source: Source) -> Option<String> {
        let _ = source;
        todo!()
    }
}
