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
//! thread with a bounded wait, like the cpal provider; at most one open is in flight. Stopping
//! cuts the stream off from the engine at once, then releases Core Audio on a worker with a
//! bounded wait, so a stalled coreaudiod never holds up the processing thread.

use super::*;
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use objc2::AnyThread;
use parking_lot::Mutex;
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
const OPEN_TIMEOUT: Duration = Duration::from_secs(2);
/// A previous open that has not returned yet (a stalled coreaudiod call) blocks new ones.
static OPENING: AtomicBool = AtomicBool::new(false);
/// How long `stop` waits for a callback holding the sender (normally free at once: a callback
/// holds it for one copy and `try_send`).
const RETIRE_WAIT: Duration = Duration::from_millis(500);
/// How long `stop` waits for Core Audio teardown before leaving it to finish in the background.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// State shared with the IOProc. Lives in an `Arc` owned by `TapStream` until the IOProc is
/// destroyed, so the raw pointer handed to Core Audio never dangles. If Core Audio fails to
/// unregister the IOProc, the `Arc` is deliberately leaked (see `stop`).
struct Ctx {
    /// This stream's only way into the engine, for the IOProc and the watcher alike. `retire`
    /// takes it out under the lock, so after that nothing from this stream reaches the engine,
    /// not even from a callback that was already running. The channel is shared with the
    /// replacement stream, so this is what keeps a late buffer out of the next one.
    tx: Mutex<Option<Sender<CaptureMsg>>>,
    dropped: Arc<AtomicU64>,
    channels: usize,
    /// The IOProc forwards audio only while set (read under the `tx` lock). Set by `open` once
    /// the caller has accepted the stream; cleared by `retire` and by the watcher on failure.
    active: AtomicBool,
    /// Set by the IOProc when the buffer layout stops matching `Opened`; the watcher reports it.
    format_changed: AtomicBool,
}

pub struct TapStream {
    dropped: Arc<AtomicU64>,
    /// Everything Core Audio holds; released when this is dropped.
    native: Native,
}

/// The tap, the aggregate device, the IOProc and the watcher. Dropping it releases them in
/// order; `TapStream::stop` does that on a worker thread.
struct Native {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    started: bool,
    ctx: Option<Arc<Ctx>>,
    stop_watch: Option<Sender<()>>,
    watcher: Option<std::thread::JoinHandle<()>>,
}

impl Native {
    fn is_released(&self) -> bool {
        self.tap == 0 && self.aggregate == 0 && self.proc_id.is_none() && self.ctx.is_none() && self.watcher.is_none()
    }
}

impl Ctx {
    fn new(tx: Sender<CaptureMsg>, dropped: Arc<AtomicU64>, channels: usize) -> Self {
        Ctx {
            tx: Mutex::new(Some(tx)),
            dropped,
            channels,
            active: AtomicBool::new(false),
            format_changed: AtomicBool::new(false),
        }
    }

    /// Runs `send` with the sender if the stream is active. Never waits: when `retire` or the
    /// watcher holds the lock, this cycle's audio is dropped (and counted).
    fn with_sender(&self, send: impl FnOnce(&Sender<CaptureMsg>)) {
        let Some(tx) = self.tx.try_lock() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if let Some(tx) = tx.as_ref()
            && self.active.load(Ordering::SeqCst)
        {
            send(tx);
        }
    }

    /// Cuts this stream off from the engine. `true`: nothing from it will be sent again.
    /// `false`: a callback has held the sender for `RETIRE_WAIT` (stalled mid-send), and that
    /// one buffer may still arrive.
    fn retire(&self) -> bool {
        self.active.store(false, Ordering::SeqCst);
        match self.tx.try_lock_for(RETIRE_WAIT) {
            Some(mut tx) => {
                *tx = None;
                true
            }
            None => false,
        }
    }
}

impl CaptureStream for TapStream {
    fn source(&self) -> Source {
        Source::System
    }
    fn dropped_buffers(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    fn stop(&mut self) {
        if self.native.is_released() {
            return;
        }
        if let Some(ctx) = &self.native.ctx
            && !ctx.retire()
        {
            tracing::error!("system audio callback stalled while stopping; one late buffer may arrive");
        }
        let native = std::mem::replace(
            &mut self.native,
            Native { tap: 0, aggregate: 0, proc_id: None, started: false, ctx: None, stop_watch: None, watcher: None },
        );
        // If the thread can't be spawned, the closure (and `native` with it) is dropped right
        // here, which releases everything on this thread instead.
        let (done, finished) = crossbeam_channel::bounded::<()>(1);
        let spawned = std::thread::Builder::new().name("tuldmi-tap-stop".into()).spawn(move || {
            drop(native);
            let _ = done.send(());
        });
        if spawned.is_ok() && finished.recv_timeout(STOP_TIMEOUT).is_err() {
            tracing::warn!("system audio is still shutting down in the background");
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
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
    ctx.with_sender(|tx| forward(ctx, tx, input));
    0
}

fn forward(ctx: &Ctx, tx: &Sender<CaptureMsg>, input: NonNull<AudioBufferList>) {
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
            return;
        }
        // SAFETY: the aggregate's input is the tap's Float32 stream (checked at open);
        // `mDataByteSize` bounds the readable bytes and Core Audio aligns sample buffers.
        planes.push(unsafe { std::slice::from_raw_parts(b.mData as *const f32, b.mDataByteSize as usize / 4) });
        counts.push(b.mNumberChannels);
    }
    match interleave(&planes, &counts, ctx.channels) {
        Some(data) if data.is_empty() => {}
        Some(data) => {
            if tx
                .try_send(CaptureMsg::Audio { source: Source::System, data, at: Instant::now() })
                .is_err()
            {
                ctx.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        None => ctx.format_changed.store(true, Ordering::Release),
    }
}

/// Opens the system tap. `tx` gets `Opened` before any `Audio`.
pub fn open(tx: Sender<CaptureMsg>) -> Result<Box<dyn CaptureStream>> {
    if OPENING.swap(true, Ordering::AcqRel) {
        return Err(crate::Error::Device("system audio is still starting".into()));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    // Zero capacity: the stream changes hands only while the caller is still waiting. After a
    // timeout the send fails and the stream is dropped (stopped) on this worker, never on the
    // caller's thread.
    let (ready, result) = crossbeam_channel::bounded::<Result<TapStream>>(0);
    let flag = cancelled.clone();
    let spawned = std::thread::Builder::new().name("tuldmi-tap-open".into()).spawn(move || {
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(tx, &flag)))
            .unwrap_or_else(|_| Err(crate::Error::Device("system audio open panicked".into())));
        OPENING.store(false, Ordering::Release);
        if let Err(crossbeam_channel::SendError(Ok(stream))) = ready.send(built) {
            drop(stream);
        }
    });
    if let Err(error) = spawned {
        OPENING.store(false, Ordering::Release);
        return Err(error.into());
    }
    match result.recv_timeout(OPEN_TIMEOUT) {
        Ok(built) => built.map(|stream| {
            // Forwarding starts only now, so a cancelled or abandoned open never delivers audio.
            if let Some(ctx) = &stream.native.ctx {
                ctx.active.store(true, Ordering::SeqCst);
            }
            Box::new(stream) as Box<dyn CaptureStream>
        }),
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            Err(crate::Error::Device("system audio open timed out".into()))
        }
    }
}

fn build(tx: Sender<CaptureMsg>, cancelled: &AtomicBool) -> Result<TapStream> {
    let dropped = Arc::new(AtomicU64::new(0));
    let mut stream = TapStream {
        dropped: dropped.clone(),
        native: Native { tap: 0, aggregate: 0, proc_id: None, started: false, ctx: None, stop_watch: None, watcher: None },
    };
    let native = &mut stream.native;
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
        check(AudioHardwareCreateProcessTap(Some(&description), &mut native.tap), "creating the system audio tap")?;

        let format = get_property(native.tap, kAudioTapPropertyFormat, std::mem::zeroed::<AudioStreamBasicDescription>())?;
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
            AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut native.aggregate)),
            "creating the system audio device",
        )?;

        // The IOProc runs at the aggregate's rate; it matches the tap's unless Core Audio says
        // otherwise. Both are watched for changes.
        let aggregate_rate = get_property(native.aggregate, kAudioDevicePropertyNominalSampleRate, 0f64)
            .ok()
            .filter(|r| plausible(*r));
        let rate = aggregate_rate.unwrap_or(format.mSampleRate);
        if !plausible(rate) {
            return Err(crate::Error::Device(format!("system audio reports an invalid sample rate ({rate})")));
        }
        tracing::info!(tap_rate = format.mSampleRate, rate, channels, "system audio tap ready");

        let ctx = Arc::new(Ctx::new(tx.clone(), dropped.clone(), channels));
        check(
            AudioDeviceCreateIOProcID(
                native.aggregate,
                Some(io_proc),
                Arc::as_ptr(&ctx) as *mut c_void,
                NonNull::from(&mut native.proc_id),
            ),
            "registering the system audio callback",
        )?;
        native.ctx = Some(ctx.clone());

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

        check(AudioDeviceStart(native.aggregate, native.proc_id), "starting system audio capture")?;
        native.started = true;
        if cancelled.load(Ordering::Acquire) {
            return Err(gave_up());
        }

        let (stop_watch, stopped) = crossbeam_channel::bounded::<()>(1);
        let rates = Rates { aggregate: native.aggregate, tap: native.tap, advertised: rate, tap_rate: format.mSampleRate };
        native.stop_watch = Some(stop_watch);
        native.watcher = Some(
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
    /// The rate sent in `Opened`: the aggregate's if readable, else the tap's.
    advertised: f64,
    tap_rate: f64,
}

/// A usable sample rate. Anything else (0 during a reconfiguration, NaN) is ignored, not acted on.
fn plausible(rate: f64) -> bool {
    rate.is_finite() && (8_000.0..=768_000.0).contains(&rate)
}

/// A plausible reading that differs from the rate in effect.
fn changed(now: Option<f64>, then: f64) -> bool {
    now.is_some_and(|now| plausible(now) && (now - then).abs() > 0.5)
}

/// Polls for a dead aggregate device (e.g. coreaudiod restarted), a sample-rate change or a
/// buffer layout change. On the first one it stops forwarding audio, then delivers `Failed`
/// (see `report_failure`).
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
                || changed(aggregate_rate, rates.advertised)
                || changed(tap_rate, rates.tap_rate)
            {
                Some("system audio format changed")
            } else {
                None
            }
        };
        if let Some(error) = failure {
            report_failure(&ctx, error, &stopped);
            return;
        }
    }
}

/// Stops forwarding audio and sends `Failed`, waiting for queue room (this is not the audio
/// callback) but never past `stop`: once the sender is retired the stream says nothing more,
/// so a late `Failed` can't reach the replacement stream. The send happens under the sender
/// lock after `active` is cleared, so no `Audio` from this stream follows it.
fn report_failure(ctx: &Ctx, error: &str, stopped: &crossbeam_channel::Receiver<()>) {
    ctx.active.store(false, Ordering::SeqCst);
    loop {
        {
            let tx = ctx.tx.lock();
            let Some(tx) = tx.as_ref() else { return };
            match tx.try_send(CaptureMsg::Failed { source: Source::System, error: error.into() }) {
                Err(crossbeam_channel::TrySendError::Full(_)) => {}
                _ => return,
            }
        }
        if !matches!(stopped.recv_timeout(Duration::from_millis(50)), Err(crossbeam_channel::RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureMsg, Ctx, Source, changed, interleave, report_failure};
    use std::sync::{Arc, atomic::AtomicU64, atomic::Ordering};
    use std::time::{Duration, Instant};

    fn audio() -> CaptureMsg {
        CaptureMsg::Audio { source: Source::System, data: vec![0.0], at: Instant::now() }
    }

    #[test]
    fn nothing_is_sent_after_retire_even_from_a_stalled_callback() {
        let (tx, rx) = crossbeam_channel::bounded(16);
        let ctx = Arc::new(Ctx::new(tx, Arc::new(AtomicU64::new(0)), 1));

        // Not accepted yet: nothing is forwarded.
        ctx.with_sender(|tx| tx.try_send(audio()).unwrap());
        assert!(rx.try_recv().is_err());
        ctx.active.store(true, Ordering::SeqCst);
        ctx.with_sender(|tx| tx.try_send(audio()).unwrap());
        assert!(matches!(rx.try_recv(), Ok(CaptureMsg::Audio { .. })));

        // A callback that passed the active check stalls before its send.
        let (entered, inside) = crossbeam_channel::bounded::<()>(0);
        let stalled = {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                ctx.with_sender(|tx| {
                    entered.send(()).unwrap();
                    std::thread::sleep(Duration::from_millis(150));
                    tx.try_send(audio()).unwrap();
                })
            })
        };
        inside.recv().unwrap();
        let started = Instant::now();
        assert!(ctx.retire());
        assert!(started.elapsed() >= Duration::from_millis(100), "retire waited for the callback");
        // Its buffer landed before retire returned, so it precedes anything a replacement sends.
        assert!(matches!(rx.try_recv(), Ok(CaptureMsg::Audio { .. })));
        stalled.join().unwrap();

        // After retire: no audio, and no late Failed either.
        ctx.active.store(true, Ordering::SeqCst);
        ctx.with_sender(|tx| tx.try_send(audio()).unwrap());
        let (_stop, stopped) = crossbeam_channel::bounded::<()>(1);
        report_failure(&ctx, "late", &stopped);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failure_waits_for_room_and_stops_audio() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let ctx = Arc::new(Ctx::new(tx, Arc::new(AtomicU64::new(0)), 1));
        ctx.active.store(true, Ordering::SeqCst);
        ctx.with_sender(|tx| tx.try_send(audio()).unwrap());
        let (_stop, stopped) = crossbeam_channel::bounded::<()>(1);
        let reporter = {
            let ctx = ctx.clone();
            std::thread::spawn(move || report_failure(&ctx, "device stopped", &stopped))
        };
        std::thread::sleep(Duration::from_millis(80));
        assert!(matches!(rx.recv().unwrap(), CaptureMsg::Audio { .. }));
        assert!(matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), CaptureMsg::Failed { .. }));
        reporter.join().unwrap();
        // Forwarding stopped with the failure.
        ctx.with_sender(|tx| tx.try_send(audio()).unwrap());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn only_plausible_rate_changes_count() {
        assert!(changed(Some(44_100.0), 48_000.0));
        assert!(!changed(Some(48_000.0), 48_000.0));
        assert!(!changed(Some(0.0), 48_000.0));
        assert!(!changed(Some(f64::NAN), 48_000.0));
        assert!(!changed(None, 48_000.0));
    }

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
