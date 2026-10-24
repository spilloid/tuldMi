//! Device smoke test: levels only, no segmentation, storage, or transcription.
use crossbeam_channel::bounded;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tuldmi_engine::capture::{
    AudioCaptureProvider, CAPTURE_CHANNEL_CAPACITY, CaptureMsg, cpal_provider::CpalProvider,
};
use tuldmi_engine::dsp::{LevelMeter, ToMono16k};
use tuldmi_engine::types::Source;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seconds = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3)
        .min(60);
    let provider = CpalProvider::new();
    for device in provider.list_devices()? {
        println!(
            "device {:?}: {} (default={})",
            device.kind, device.name, device.is_default
        );
    }
    let (tx, rx) = bounded(CAPTURE_CHANNEL_CAPACITY);
    let mut streams = Vec::new();
    for source in [Source::Mic, Source::System] {
        match provider.open(source, None, tx.clone()) {
            Ok(stream) => streams.push(stream),
            Err(error) => eprintln!("{source:?}: {error}"),
        }
    }
    let mut pipelines = HashMap::new();
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut last = Instant::now();
    while Instant::now() < end {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(CaptureMsg::Opened {
                source,
                device_name,
                sample_rate,
                channels,
                ..
            }) => {
                println!("opened {source:?}: {device_name}, {sample_rate} Hz, {channels} ch");
                pipelines.insert(
                    source,
                    (ToMono16k::new(sample_rate, channels)?, LevelMeter::new()),
                );
            }
            Ok(CaptureMsg::Failed { source, error }) => eprintln!("failed {source:?}: {error}"),
            Ok(CaptureMsg::Audio { source, data, .. }) => {
                if let Some((convert, meter)) = pipelines.get_mut(&source) {
                    let mut mono = Vec::new();
                    convert.process(&data, &mut mono)?;
                    let mut levels = Vec::new();
                    meter.push(&mono, &mut levels);
                    if last.elapsed() >= Duration::from_millis(100)
                        && let Some((rms, peak)) = levels.last()
                    {
                        println!("{source:?}: rms={rms:.4}, peak={peak:.4}");
                        last = Instant::now();
                    }
                }
            }
            Err(_) => (),
        }
    }
    for stream in &mut streams {
        stream.stop();
        println!(
            "stopped {:?}: {} dropped buffers",
            stream.source(),
            stream.dropped_buffers()
        );
    }
    Ok(())
}
