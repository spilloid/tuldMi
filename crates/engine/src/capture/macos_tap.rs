//! macOS system audio through a Core Audio process tap (macOS 14.4+).
//!
//! A private, global tap (the output of every process, mixed down to mono, playback left
//! untouched) is the only member of a private aggregate device, and an IOProc on that device
//! receives the tapped audio. The tap belongs to no output device, so switching speakers or
//! headphones needs no reopen, and there is exactly one "device" to offer.
//!
//! The first start asks for System Audio Recording permission (Info.plist
//! `NSAudioCaptureUsageDescription`). If the user denies it, macOS delivers silence rather than an
//! error; the UI and docs say where to grant it.
//!
//! Callback contract (see `capture`): the IOProc copies the buffer and `try_send`s it, never
//! blocks. A watcher thread polls for a dead aggregate device, a sample-rate change or a buffer
//! layout change; on any of them it stops forwarding audio (so nothing is passed on under the old
//! format beyond one 500 ms poll) and reports `CaptureMsg::Failed`, and the engine reopens.
//!
//! Opening talks to coreaudiod (and may trigger the permission prompt), so it runs on its own
//! thread with a bounded wait, like the cpal provider; at most one open is in flight.

use super::*;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::{
    AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart,
    AudioDeviceStop, AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertySelector, CATapDescription,
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
    kAudioTapPropertyFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
    kAudioFormatFlagIsBigEndian, kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved,
    kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use crate::types::{DeviceInfo, DeviceKind};

/// The single System source on macOS. Stable, so default-device following never reopens it.
pub const DEVICE_ID: &str = "macos-system-audio";
pub const DEVICE_NAME: &str = "All system audio";

pub fn device_info() -> DeviceInfo {
    DeviceInfo {
        id: DEVICE_ID.into(),
        name: DEVICE_NAME.into(),
        kind: DeviceKind::Output,
        is_default: true,
    }
}

type OSStatus = i32;

fn check(status: OSStatus, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(crate::Error::Device(format!("{what} failed (OSStatus {status} / {})", fourcc(status))))
    }
}

/// Core Audio errors are usually four-character codes ('!obj', 'who?').
fn fourcc(status: OSStatus) -> String {
    let bytes = status.to_be_bytes();
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        status.to_string()
    }
}

fn address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property. `T` must be the property's plain-data type.
unsafe fn get_property<T: Copy>(object: AudioObjectID, selector: AudioObjectPropertySelector, mut value: T) -> Result<T> {
    let mut addr = address(selector);
    let mut size = std::mem::size_of::<T>() as u32;
    // SAFETY: `addr`, `size` and `value` are live locals; `T` matches the property's layout
    // (caller's contract) and `size` bounds the write.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, "AudioObjectGetPropertyData")?;
    if size as usize != std::mem::size_of::<T>() {
        return Err(crate::Error::Device("unexpected Core Audio property size".into()));
    }
    Ok(value)
}

fn ns(key: &CStr) -> Retained<NSString> {
    NSString::from_str(key.to_str().unwrap_or_default())
}

/// How long `open` waits for coreaudiod before giving up (the engine retries every 3 s).
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
/// A previous open that has not returned yet (a stalled coreaudiod call) blocks new ones.
static OPENING: AtomicBool = AtomicBool::new(false);

/// State shared with the IOProc. Lives in an `Arc` owned by `TapStream` until the IOProc is
/// destroyed, so the raw pointer handed to Core Audio never dangles. If Core Audio fails to
/// unregister the IOProc, the `Arc` is deliberately leaked (see `stop`).
struct Ctx {
    tx: Sender<CaptureMsg>,
    dropped: Arc<AtomicU64>,
    channels: usize,
    /// The IOProc forwards audio only while set. Cleared by `stop` and by the watcher on failure.
    active: AtomicBool,
    /// Set by the IOProc when the buffer layout stops matching `Opened`; the watcher reports it.
    format_changed: AtomicBool,
}

pub struct TapStream {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    started: bool,
    ctx: Option<Arc<Ctx>>,
    dropped: Arc<AtomicU64>,
    stop_watch: Option<Sender<()>>,
    watcher: Option<std::thread::JoinHandle<()>>,
}

impl CaptureStream for TapStream {
    fn source(&self) -> Source {
        Source::System
    }
    fn dropped_buffers(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    fn stop(&mut self) {
        if let Some(ctx) = &self.ctx {
            ctx.active.store(false, Ordering::Release);
        }
        if let Some(stop) = self.stop_watch.take() {
            let _ = stop.try_send(());
        }
        if let Some(watcher) = self.watcher.take()
            && watcher.join().is_err()
        {
            tracing::error!("system tap watcher panicked");
        }
        // A successful AudioDeviceStop + DestroyIOProcID, called off the IO thread, returns only
        // after the IOProc has finished. A failed one guarantees nothing, so then the callback
        // state is kept alive (inactive) for the rest of the process instead of being freed.
        let mut quiescent = true;
        // SAFETY: each id was returned by the matching create call and is released once (fields
        // are cleared).
        unsafe {
            if self.proc_id.is_some() {
                if self.started {
                    let status = AudioDeviceStop(self.aggregate, self.proc_id);
                    quiescent &= status == 0;
                    self.started = false;
                }
                let status = AudioDeviceDestroyIOProcID(self.aggregate, self.proc_id);
                quiescent &= status == 0;
                self.proc_id = None;
            }
            if self.aggregate != 0 {
                let _ = AudioHardwareDestroyAggregateDevice(self.aggregate);
                self.aggregate = 0;
            }
            if self.tap != 0 {
                let _ = AudioHardwareDestroyProcessTap(self.tap);
                self.tap = 0;
            }
        }
        if let Some(ctx) = self.ctx.take()
            && !quiescent
        {
            tracing::error!("system audio callback did not unregister cleanly; keeping its state alive");
            std::mem::forget(ctx);
        }
    }
}

impl Drop for TapStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Copies one IO cycle into interleaved f32. Handles one interleaved buffer or one buffer per
/// channel. `None` when the layout doesn't carry `channels` channels (format changed).
fn interleave(buffers: &[&[f32]], per_buffer_channels: &[u32], channels: usize) -> Option<Vec<f32>> {
    match buffers {
        [] => Some(Vec::new()),
        [only] if per_buffer_channels.first().copied() == Some(channels as u32) && only.len() % channels == 0 => {
            Some(only.to_vec())
        }
        planes if planes.len() == channels && per_buffer_channels.iter().all(|&c| c == 1) => {
            let frames = planes.iter().map(|p| p.len()).min().unwrap_or(0);
            let mut out = Vec::with_capacity(frames * channels);
            for i in 0..frames {
                out.extend(planes.iter().map(|p| p[i]));
            }
            Some(out)
        }
        _ => None,
    }
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> OSStatus {
    if client.is_null() {
        return 0;
    }
    // SAFETY: `client` is `Arc::as_ptr` of the `Ctx` that `TapStream` keeps alive until this
    // IOProc is destroyed; only shared access happens here.
    let ctx = unsafe { &*(client as *const Ctx) };
    if !ctx.active.load(Ordering::Acquire) {
        return 0;
    }
    let list = input.as_ptr();
    // SAFETY: Core Audio passes a valid AudioBufferList whose trailing array holds
    // `mNumberBuffers` entries (the struct declares `[AudioBuffer; 1]` as a C flexible array).
    let raw: &[AudioBuffer] = unsafe {
        let n = (*list).mNumberBuffers as usize;
        std::slice::from_raw_parts((&raw const (*list).mBuffers).cast::<AudioBuffer>(), n)
    };
    let mut planes = Vec::with_capacity(raw.len());
    let mut counts = Vec::with_capacity(raw.len());
    for b in raw {
        if b.mData.is_null() {
            return 0;
        }
        // SAFETY: the aggregate's input is the tap's Float32 stream (checked at open);
        // `mDataByteSize` bounds the readable bytes and Core Audio aligns sample buffers.
        planes.push(unsafe { std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4) });
        counts.push(b.mNumberChannels);
    }
    match interleave(&planes, &counts, ctx.channels) {
        Some(data) if data.is_empty() => {}
        Some(data) => {
            if ctx
                .tx
                .try_send(CaptureMsg::Audio { source: Source::System, data, at: Instant::now() })
                .is_err()
            {
                ctx.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        None => ctx.format_changed.store(true, Ordering::Release),
    }
    0
}

/// Opens the system tap. `tx` gets `Opened` before any `Audio`.
pub fn open(tx: Sender<CaptureMsg>) -> Result<Box<dyn CaptureStream>> {
    if OPENING.swap(true, Ordering::AcqRel) {
        return Err(crate::Error::Device("system audio is still starting".into()));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let (ready, result) = crossbeam_channel::bounded::<Result<TapStream>>(1);
    let flag = cancelled.clone();
    let spawned = std::thread::Builder::new().name("tuldmi-tap-open".into()).spawn(move || {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(tx, &flag)))
            .unwrap_or_else(|_| Err(crate::Error::Device("system audio open panicked".into())));
        OPENING.store(false, Ordering::Release);
        // If the caller already gave up, the stream is dropped (and stopped) right here or with
        // the channel.
        let _ = ready.send(built);
    });
    if let Err(error) = spawned {
        OPENING.store(false, Ordering::Release);
        return Err(error.into());
    }
    match result.recv_timeout(OPEN_TIMEOUT) {
        Ok(built) => built.map(|stream| Box::new(stream) as Box<dyn CaptureStream>),
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            Err(crate::Error::Device("system audio open timed out".into()))
        }
    }
}

fn build(tx: Sender<CaptureMsg>, cancelled: &AtomicBool) -> Result<TapStream> {
    let dropped = Arc::new(AtomicU64::new(0));
    let mut stream = TapStream {
        tap: 0,
        aggregate: 0,
        proc_id: None,
        started: false,
        ctx: None,
        dropped: dropped.clone(),
        stop_watch: None,
        watcher: None,
    };
    // Any early return drops `stream`, which releases whatever was created so far.
    let gave_up = || crate::Error::Device("system audio open cancelled".into());

    // SAFETY (whole block): plain Core Audio / Foundation calls on objects created here; every
    // out-pointer is a live local.
    unsafe {
        let description = CATapDescription::initMonoGlobalTapButExcludeProcesses(
            CATapDescription::alloc(),
            &NSArray::<NSNumber>::new(),
        );
        description.setName(&NSString::from_str("tuldMi system audio"));
        description.setPrivate(true);
        let tap_uid = description.UUID().UUIDString();
        check(AudioHardwareCreateProcessTap(Some(&description), &mut stream.tap), "creating the system audio tap")?;

        let format = get_property(stream.tap, kAudioTapPropertyFormat, std::mem::zeroed::<AudioStreamBasicDescription>())?;
        let channels = format.mChannelsPerFrame as usize;
        let non_interleaved = format.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0;
        let frame_bytes = if non_interleaved { 4 } else { 4 * format.mChannelsPerFrame };
        if format.mFormatID != kAudioFormatLinearPCM
            || format.mFormatFlags & kAudioFormatFlagIsFloat == 0
            || format.mFormatFlags & kAudioFormatFlagIsBigEndian != 0
            || format.mBitsPerChannel != 32
            || channels == 0
            || format.mBytesPerFrame != frame_bytes
        {
            return Err(crate::Error::Device(format!(
                "unsupported system tap format (id {}, flags {:#x}, {} bits, {} ch, {} bytes/frame)",
                fourcc(format.mFormatID as i32),
                format.mFormatFlags,
                format.mBitsPerChannel,
                format.mChannelsPerFrame,
                format.mBytesPerFrame
            )));
        }

        let sub_tap: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[&*ns(kAudioSubTapUIDKey), &*ns(kAudioSubTapDriftCompensationKey)],
            &[&*tap_uid as &AnyObject, &*NSNumber::new_bool(true) as &AnyObject],
        );
        let taps = NSArray::from_slice(&[&*sub_tap]);
        let aggregate_uid = NSString::from_str(&format!("com.spillerstech.tuldmi.tap.{tap_uid}"));
        let name = NSString::from_str("tuldMi system audio");
        let yes = NSNumber::new_bool(true);
        let config: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[
                &*ns(kAudioAggregateDeviceNameKey),
                &*ns(kAudioAggregateDeviceUIDKey),
                &*ns(kAudioAggregateDeviceIsPrivateKey),
                &*ns(kAudioAggregateDeviceTapAutoStartKey),
                &*ns(kAudioAggregateDeviceTapListKey),
            ],
            &[&*name as &AnyObject, &*aggregate_uid as &AnyObject, &*yes as &AnyObject, &*yes as &AnyObject, &*taps as &AnyObject],
        );
        // NSDictionary is toll-free bridged to CFDictionary.
        let cf = &*(Retained::as_ptr(&config) as *const CFDictionary);
        check(
            AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut stream.aggregate)),
            "creating the system audio device",
        )?;

        // The IOProc runs at the aggregate's rate; it matches the tap's unless Core Audio says
        // otherwise. Both are watched for changes.
        let aggregate_rate = get_property(stream.aggregate, kAudioDevicePropertyNominalSampleRate, 0f64)
            .ok()
            .filter(|r| *r >= 8_000.0);
        let rate = aggregate_rate.unwrap_or(format.mSampleRate);
        if rate < 8_000.0 {
            return Err(crate::Error::Device(format!("system audio reports an invalid sample rate ({rate})")));
        }
        tracing::info!(tap_rate = format.mSampleRate, rate, channels, "system audio tap ready");

        let ctx = Arc::new(Ctx {
            tx: tx.clone(),
            dropped: dropped.clone(),
            channels,
            active: AtomicBool::new(true),
            format_changed: AtomicBool::new(false),
        });
        check(
            AudioDeviceCreateIOProcID(
                stream.aggregate,
                Some(io_proc),
                Arc::as_ptr(&ctx) as *mut c_void,
                NonNull::from(&mut stream.proc_id),
            ),
            "registering the system audio callback",
        )?;
        stream.ctx = Some(ctx.clone());

        if cancelled.load(Ordering::Acquire) {
            return Err(gave_up());
        }
        tx.try_send(CaptureMsg::Opened {
            source: Source::System,
            device_id: DEVICE_ID.into(),
            device_name: DEVICE_NAME.into(),
            sample_rate: rate.round() as u32,
            channels: channels as u16,
        })
        .map_err(|_| crate::Error::Device("capture queue full at open".into()))?;

        check(AudioDeviceStart(stream.aggregate, stream.proc_id), "starting system audio capture")?;
        stream.started = true;
        if cancelled.load(Ordering::Acquire) {
            return Err(gave_up());
        }

        let (stop_watch, stopped) = crossbeam_channel::bounded::<()>(1);
        let rates = Rates { aggregate: stream.aggregate, tap: stream.tap, aggregate_rate, tap_rate: format.mSampleRate };
        stream.stop_watch = Some(stop_watch);
        stream.watcher = Some(
            std::thread::Builder::new()
                .name("tuldmi-tap-watch".into())
                .spawn(move || watch(rates, ctx, stopped))?,
        );
    }
    Ok(stream)
}

/// What the watcher compares against: the rates in effect when `Opened` was sent.
struct Rates {
    aggregate: AudioObjectID,
    tap: AudioObjectID,
    aggregate_rate: Option<f64>,
    tap_rate: f64,
}

fn changed(now: f64, then: f64) -> bool {
    (now - then).abs() > 0.5
}

/// Polls for a dead aggregate device (e.g. coreaudiod restarted), a sample-rate change or a
/// buffer layout change. On the first one it stops forwarding audio, then delivers `Failed`,
/// waiting for queue room (it is not the audio callback) unless the stream is stopped first.
fn watch(rates: Rates, ctx: Arc<Ctx>, stopped: crossbeam_channel::Receiver<()>) {
    loop {
        match stopped.recv_timeout(Duration::from_millis(500)) {
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
        // SAFETY: both objects stay valid until `stop` joins this thread before destroying them.
        let failure = unsafe {
            let alive = get_property(rates.aggregate, kAudioDevicePropertyDeviceIsAlive, 0u32);
            let aggregate_rate = get_property(rates.aggregate, kAudioDevicePropertyNominalSampleRate, 0f64).ok();
            let tap_rate = get_property(rates.tap, kAudioTapPropertyFormat, std::mem::zeroed::<AudioStreamBasicDescription>())
                .ok()
                .map(|f| f.mSampleRate);
            if !matches!(alive, Ok(n) if n != 0) {
                Some("system audio device stopped")
            } else if ctx.format_changed.load(Ordering::Acquire)
                || matches!((aggregate_rate, rates.aggregate_rate), (Some(now), Some(then)) if changed(now, then))
                || tap_rate.is_some_and(|now| changed(now, rates.tap_rate))
            {
                Some("system audio format changed")
            } else {
                None
            }
        };
        if let Some(error) = failure {
            ctx.active.store(false, Ordering::Release);
            crossbeam_channel::select! {
                send(ctx.tx, CaptureMsg::Failed { source: Source::System, error: error.into() }) -> _ => (),
                recv(stopped) -> _ => (),
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::interleave;

    #[test]
    fn interleaved_buffer_is_copied() {
        let a = [0.1, 0.2, 0.3, 0.4];
        assert_eq!(interleave(&[&a], &[2], 2), Some(a.to_vec()));
    }

    #[test]
    fn planar_buffers_are_interleaved_to_the_shortest() {
        let l = [1.0, 2.0, 3.0];
        let r = [-1.0, -2.0];
        assert_eq!(interleave(&[&l, &r], &[1, 1], 2), Some(vec![1.0, -1.0, 2.0, -2.0]));
    }

    #[test]
    fn layout_mismatch_is_reported() {
        let a = [0.0; 4];
        assert_eq!(interleave(&[&a], &[2], 1), None);
        // A partial frame means the byte size no longer matches the channel count.
        assert_eq!(interleave(&[&[0.0; 3]], &[2], 2), None);
        assert_eq!(interleave(&[&a, &a, &a], &[1, 1, 1], 2), None);
        assert_eq!(interleave(&[], &[], 1), Some(vec![]));
    }
}
