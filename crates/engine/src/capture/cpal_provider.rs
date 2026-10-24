//! cpal capture. On WASAPI, input on a render endpoint enables LOOPBACK.
//! Use its default OUTPUT config: default_input_config rejects render endpoints
//! in cpal 0.18.2. Stream construction, playback, and destruction share one thread.
use super::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

pub struct CpalProvider;
impl CpalProvider {
    pub fn new() -> Self {
        Self
    }
}
impl Default for CpalProvider {
    fn default() -> Self {
        Self::new()
    }
}
fn device_id(device: &cpal::Device) -> String {
    device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| device.to_string())
}
fn default_device(host: &cpal::Host, source: Source) -> Option<cpal::Device> {
    match source {
        Source::Mic => host.default_input_device(),
        Source::System => host.default_output_device(),
    }
}
struct OwnedStream {
    source: Source,
    stop: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}
impl CaptureStream for OwnedStream {
    fn source(&self) -> Source {
        self.source
    }
    fn dropped_buffers(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    fn stop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("capture owner thread failed");
        }
    }
}
impl Drop for OwnedStream {
    fn drop(&mut self) {
        self.stop();
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    source: Source,
    tx: Sender<CaptureMsg>,
    dropped: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let errors = tx.clone();
    // WASAPI reports `Xrun` for DATA_DISCONTINUITY, which loopback raises routinely whenever
    // playback starts or stops. It is not lost-to-us audio, so it is logged, not surfaced.
    let xruns = Arc::new(AtomicU64::new(0));
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data, _| {
                let data = data
                    .iter()
                    .map(|&v| <f32 as cpal::FromSample<T>>::from_sample_(v))
                    .collect();
                if tx
                    .try_send(CaptureMsg::Audio {
                        source,
                        data,
                        at: Instant::now(),
                    })
                    .is_err()
                {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            },
            move |error| {
                // An xrun is a glitch, not a dead stream: the device keeps delivering audio.
                // Tearing the stream down here (and reopening seconds later) loses far more
                // speech than the glitch itself — found in live testing under ASR load.
                if error.kind() == cpal::ErrorKind::Xrun {
                    let n = xruns.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_power_of_two() {
                        tracing::debug!(source = source.as_str(), xruns = n, "capture discontinuity");
                    }
                    return;
                }
                failed.store(true, Ordering::Release);
                let _ = errors.try_send(CaptureMsg::Failed {
                    source,
                    error: error.to_string(),
                });
            },
            Some(Duration::from_secs(1)),
        )
        .map_err(|e| crate::Error::Device(e.to_string()))
}
impl AudioCaptureProvider for CpalProvider {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let mut out = Vec::new();
        for source in [Source::Mic, Source::System] {
            if source == Source::System && !cfg!(target_os = "windows") {
                continue;
            }
            let default_id = self.default_device_id(source);
            let devices: Vec<_> = match source {
                Source::Mic => host.input_devices().map(|d| d.collect()),
                Source::System => host.output_devices().map(|d| d.collect()),
            }
            .map_err(|e| crate::Error::Device(e.to_string()))?;
            for device in devices {
                let id = device_id(&device);
                out.push(DeviceInfo {
                    is_default: default_id.as_ref() == Some(&id),
                    id,
                    name: device.to_string(),
                    kind: if source == Source::Mic {
                        crate::types::DeviceKind::Input
                    } else {
                        crate::types::DeviceKind::Output
                    },
                });
            }
        }
        Ok(out)
    }
    fn open(
        &self,
        source: Source,
        selected: Option<&str>,
        tx: Sender<CaptureMsg>,
    ) -> Result<Box<dyn CaptureStream>> {
        if source == Source::System && !cfg!(target_os = "windows") {
            return Err(crate::Error::Device(
                "system capture is supported on Windows only".into(),
            ));
        }
        let selected = selected.map(str::to_owned);
        let (stop, stopped) = crossbeam_channel::bounded(1);
        let (ready, opened) = crossbeam_channel::bounded(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let count = dropped.clone();
        let thread = std::thread::Builder::new()
            .name(format!("tuldmi-cpal-{}", source.as_str()))
            .spawn(move || {
                let run = || -> Result<()> {
                    let host = cpal::default_host();
                    let device = if let Some(id) = selected {
                        host.devices()
                            .map_err(|e| crate::Error::Device(e.to_string()))?
                            .find(|d| device_id(d) == id)
                    } else {
                        default_device(&host, source)
                    }
                    .ok_or_else(|| crate::Error::Device("audio device unavailable".into()))?;
                    let native = match source {
                        Source::Mic => device.default_input_config(),
                        Source::System => device.default_output_config(),
                    }
                    .map_err(|e| crate::Error::Device(e.to_string()))?;
                    let config = native.config();
                    let failed = Arc::new(AtomicBool::new(false));
                    // Prefer f32 at the native rate/channel count; fall back to the native format.
                    let make = |format| match format {
                        cpal::SampleFormat::F32 => build::<f32>(
                            &device,
                            &config,
                            source,
                            tx.clone(),
                            count.clone(),
                            failed.clone(),
                        ),
                        cpal::SampleFormat::I16 => build::<i16>(
                            &device,
                            &config,
                            source,
                            tx.clone(),
                            count.clone(),
                            failed.clone(),
                        ),
                        cpal::SampleFormat::I32 => build::<i32>(
                            &device,
                            &config,
                            source,
                            tx.clone(),
                            count.clone(),
                            failed.clone(),
                        ),
                        cpal::SampleFormat::U16 => build::<u16>(
                            &device,
                            &config,
                            source,
                            tx.clone(),
                            count.clone(),
                            failed.clone(),
                        ),
                        _ => Err(crate::Error::Device(
                            "unsupported native sample format".into(),
                        )),
                    };
                    let stream =
                        make(cpal::SampleFormat::F32).or_else(|_| make(native.sample_format()))?;
                    if stopped.try_recv().is_ok() {
                        return Err(crate::Error::Device("audio open cancelled".into()));
                    }
                    tx.try_send(CaptureMsg::Opened {
                        source,
                        device_id: device_id(&device),
                        device_name: device.to_string(),
                        sample_rate: config.sample_rate,
                        channels: config.channels,
                    })
                    .map_err(|_| crate::Error::Device("capture queue full at open".into()))?;
                    stream
                        .play()
                        .map_err(|e| crate::Error::Device(e.to_string()))?;
                    let _ = ready.try_send(Ok(()));
                    while let Err(crossbeam_channel::RecvTimeoutError::Timeout) =
                        stopped.recv_timeout(Duration::from_millis(50))
                    {
                        // Retry a failure notification if the callback's queue was full.
                        if failed.load(Ordering::Acquire)
                            && tx
                                .try_send(CaptureMsg::Failed {
                                    source,
                                    error: "audio stream failed".into(),
                                })
                                .is_ok()
                        {
                            break;
                        }
                    }
                    drop(stream);
                    Ok(())
                };
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
                    Ok(Ok(())) => (),
                    Ok(Err(error)) => { let _ = ready.try_send(Err(error)); }
                    Err(_) => {
                        tracing::error!("capture owner thread panicked");
                        let _ = ready.try_send(Err(crate::Error::Device("capture owner thread panicked".into())));
                        // This is the owner thread, not the audio callback. Wait
                        // for room or stop so a full capture queue cannot hide failure.
                        crossbeam_channel::select! {
                            send(tx, CaptureMsg::Failed { source, error: "capture owner thread panicked".into() }) -> _ => (),
                            recv(stopped) -> _ => (),
                        }
                    }
                }
            })?;
        match opened.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Box::new(OwnedStream {
                source,
                stop,
                thread: Some(thread),
                dropped,
            })),
            result => {
                let _ = stop.try_send(());
                // A driver that ignores activation_timeout must not block the caller forever.
                if thread.is_finished() && thread.join().is_err() {
                    tracing::error!("capture open thread failed");
                }
                Err(match result {
                    Ok(Err(e)) => e,
                    _ => crate::Error::Device("audio device open timed out".into()),
                })
            }
        }
    }
    fn default_device_id(&self, source: Source) -> Option<String> {
        let host = cpal::default_host();
        let default = default_device(&host, source)?;
        // WASAPI resolves symbolic defaults in Device::id().
        Some(device_id(&default))
    }
}
