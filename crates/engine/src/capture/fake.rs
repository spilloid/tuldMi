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
    /// drops) - compresses wall-clock timestamps, fine for logic tests.
    pub realtime: bool,
    /// After this many buffers, send `Failed` and stop producing (simulates unplugging). Only
    /// the first stream opened for this source fails; reopening yields a healthy stream.
    pub fail_after: Option<usize>,
    pub device_name: String,
    /// After `samples` is exhausted: `true` = keep sending digital silence; `false` = send
    /// nothing at all, like WASAPI loopback while nothing is playing.
    pub silence_when_exhausted: bool,
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
            silence_when_exhausted: true,
        }
    }
}

pub struct FakeProvider {
    sources: std::collections::HashMap<Source, FakeSource>,
    counts: parking_lot::Mutex<std::collections::HashMap<Source, usize>>,
    started: parking_lot::Mutex<std::collections::HashSet<Source>>,
}
impl FakeProvider {
    pub fn new() -> Self {
        Self {
            sources: Default::default(),
            counts: Default::default(),
            started: Default::default(),
        }
    }
    pub fn with_source(mut self, source: Source, fake: FakeSource) -> Self {
        self.sources.insert(source, fake);
        self
    }
    /// How many times open has been called for this source, including failed attempts.
    pub fn open_count(&self, source: Source) -> usize {
        self.counts.lock().get(&source).copied().unwrap_or(0)
    }
    /// Silence-buffer interval after a non-realtime source has exhausted its samples.
    pub const IDLE_TICK: Duration = Duration::from_millis(10);
}
impl Default for FakeProvider {
    fn default() -> Self {
        Self::new()
    }
}
struct FakeStream {
    source: Source,
    stop: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl CaptureStream for FakeStream {
    fn source(&self) -> Source {
        self.source
    }
    fn dropped_buffers(&self) -> u64 {
        0
    }
    fn stop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("fake capture thread failed");
        }
    }
}
impl Drop for FakeStream {
    fn drop(&mut self) {
        self.stop();
    }
}
impl AudioCaptureProvider for FakeProvider {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        Ok([Source::Mic, Source::System]
            .into_iter()
            .filter_map(|source| {
                self.sources.get(&source).map(|fake| DeviceInfo {
                    id: format!("fake-{}", source.as_str()),
                    name: fake.device_name.clone(),
                    kind: if source == Source::Mic {
                        crate::types::DeviceKind::Input
                    } else {
                        crate::types::DeviceKind::Output
                    },
                    is_default: true,
                })
            })
            .collect())
    }
    fn open(
        &self,
        source: Source,
        device_id: Option<&str>,
        tx: Sender<CaptureMsg>,
    ) -> Result<Box<dyn CaptureStream>> {
        *self.counts.lock().entry(source).or_default() += 1;
        let fake = self
            .sources
            .get(&source)
            .cloned()
            .ok_or_else(|| crate::Error::Device("fake device unavailable".into()))?;
        let id = format!("fake-{}", source.as_str());
        if device_id.is_some_and(|wanted| wanted != id)
            || fake.sample_rate == 0
            || fake.channels == 0
            || fake.buffer_frames == 0
        {
            return Err(crate::Error::Device("invalid fake device or format".into()));
        }
        let first = self.started.lock().insert(source);
        let (stop, stopped) = crossbeam_channel::bounded(1);
        let thread = std::thread::Builder::new()
            .name(format!("tuldmi-fake-{}", source.as_str()))
            .spawn(move || {
                let send = |msg| {
                    crossbeam_channel::select! {
                        send(tx, msg) -> result => result.is_ok(), recv(stopped) -> _ => false,
                    }
                };
                if !send(CaptureMsg::Opened {
                    source,
                    device_id: id,
                    device_name: fake.device_name,
                    sample_rate: fake.sample_rate,
                    channels: fake.channels,
                }) {
                    return;
                }
                let width = fake.buffer_frames.saturating_mul(fake.channels as usize);
                let mut offset = 0;
                let mut count = 0;
                let mut next_buffer = Instant::now();
                loop {
                    if first && fake.fail_after.is_some_and(|n| count >= n) {
                        send(CaptureMsg::Failed {
                            source,
                            error: "fake device disconnected".into(),
                        });
                        return;
                    }
                    let exhausted = offset >= fake.samples.len();
                    if exhausted && !fake.silence_when_exhausted {
                        // Real loopback goes quiet: no buffers until something plays again.
                        // Park until the stream is stopped.
                        let _ = stopped.recv();
                        return;
                    }
                    let data = if exhausted {
                        vec![0.0; width]
                    } else {
                        let end = offset.saturating_add(width).min(fake.samples.len());
                        let data = fake.samples[offset..end].to_vec();
                        offset = end;
                        data
                    };
                    let delay = if fake.realtime {
                        next_buffer += Duration::from_secs_f64(
                            fake.buffer_frames as f64 / fake.sample_rate as f64,
                        );
                        next_buffer.saturating_duration_since(Instant::now())
                    } else if exhausted {
                        FakeProvider::IDLE_TICK
                    } else {
                        Duration::ZERO
                    };
                    if !delay.is_zero()
                        && stopped.recv_timeout(delay)
                            != Err(crossbeam_channel::RecvTimeoutError::Timeout)
                    {
                        return;
                    }
                    if !send(CaptureMsg::Audio {
                        source,
                        data,
                        at: Instant::now(),
                    }) {
                        return;
                    }
                    count += 1;
                }
            })?;
        Ok(Box::new(FakeStream {
            source,
            stop,
            thread: Some(thread),
        }))
    }
    fn default_device_id(&self, source: Source) -> Option<String> {
        self.sources
            .contains_key(&source)
            .then(|| format!("fake-{}", source.as_str()))
    }
}
