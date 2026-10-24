//! Private runtime actors. Codec and ASR work are isolated from capture processing.
use super::*;
use crate::capture::{CAPTURE_CHANNEL_CAPACITY, CaptureMsg, CaptureStream};
use crate::clock::{StreamClock, WallAnchor};
use crate::dsp::{LevelMeter, ToMono16k};
use crate::segmenter::{FRAME_SAMPLES, Segmenter, SegmenterConfig, SpeechSegment};
use crate::storage::{NewAudio, NewEvent};
use crate::vad::{SpeechDetector, best_detector};
use crossbeam_channel::{Receiver, TrySendError};
use std::collections::{HashMap, VecDeque};

pub(super) struct Shared {
    pub store: Arc<Store>,
    pub sink: Arc<dyn EventSink>,
    pub provider: Arc<dyn AudioCaptureProvider>,
    pub settings: Mutex<Settings>,
    pub status: Mutex<CaptureStatus>,
    pub config: EngineConfig,
    pub quit: AtomicBool,
    pub cancel_asr: Arc<AtomicBool>,
    pub wake: Sender<()>,
    pub persist_gate: Mutex<()>,
    published: Mutex<(Instant, Option<CaptureStatus>)>,
    wall: WallAnchor,
}
impl Shared {
    pub fn new(
        config: EngineConfig,
        store: Arc<Store>,
        sink: Arc<dyn EventSink>,
        provider: Arc<dyn AudioCaptureProvider>,
        settings: Settings,
        wake: Sender<()>,
    ) -> Result<Self> {
        let backlog = store.backlog()?;
        let sources = [Source::Mic, Source::System]
            .map(|source| SourceStatus {
                source,
                enabled: enabled(&settings, source),
                state: SourceState::Disabled,
                device_id: None,
                device_name: None,
                format: None,
                error: None,
            })
            .to_vec();
        Ok(Self {
            store,
            sink,
            provider,
            settings: Mutex::new(settings),
            status: Mutex::new(CaptureStatus {
                state: CaptureState::Stopped,
                session_id: None,
                session_started_at: None,
                sources,
                backlog,
                model_id: None,
                transcriber: TranscriberState::NoModel,
            }),
            config,
            quit: AtomicBool::new(false),
            cancel_asr: Arc::new(AtomicBool::new(false)),
            wake,
            persist_gate: Mutex::new(()),
            published: Mutex::new((Instant::now() - Duration::from_secs(1), None)),
            wall: WallAnchor::now(),
        })
    }
    pub fn emit(&self, event: EngineEvent) {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sink.emit(event))).is_err()
        {
            tracing::error!("event sink panicked");
        }
    }
    pub fn publish(&self) {
        let status = self.status.lock().clone();
        let mut published = self.published.lock();
        if published.0.elapsed() >= Duration::from_millis(100)
            && published.1.as_ref() != Some(&status)
        {
            *published = (Instant::now(), Some(status.clone()));
            drop(published);
            self.emit(EngineEvent::Status(status));
        }
    }
    pub fn notice(&self, message: &str) {
        tracing::warn!(message, "engine notice");
        self.emit(EngineEvent::Notice {
            level: NoticeLevel::Warning,
            message: message.into(),
        });
    }
    pub fn upsert(&self, id: Uuid) -> Result<()> {
        if let Some(event) = self.store.get_event(id)? {
            self.emit(EngineEvent::EventUpserted(event));
        }
        Ok(())
    }
    pub fn refresh_backlog(&self) {
        match self.store.backlog() {
            Ok(n) => self.status.lock().backlog = n,
            Err(_) => self.notice("could not read transcription backlog"),
        }
        self.publish();
    }
}
pub(super) fn spawn(
    name: &str,
    shared: Arc<Shared>,
    run: impl FnOnce(Arc<Shared>) + Send + 'static,
) -> Result<std::thread::JoinHandle<()>> {
    let worker = name.to_owned();
    Ok(std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let report = shared.clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(shared))).is_err() {
                report.notice("engine worker panicked; other workers remain running");
                if worker == "tuldmi-asr" {
                    report.status.lock().transcriber = TranscriberState::Error;
                }
                if worker == "tuldmi-proc" {
                    let mut status = report.status.lock();
                    status.state = CaptureState::Error;
                    for source in status.sources.iter_mut().filter(|s| s.enabled) {
                        source.state = SourceState::Unavailable;
                        source.error =
                            Some("capture processing worker failed; restart the engine".into());
                    }
                }
                report.publish();
            }
        })?)
}
pub(super) fn unlink(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
/// Repair the filesystem/SQLite rename boundary after an interrupted finalization.
pub(super) fn recover_audio_names(store: &Store) -> Result<()> {
    let mut db: Option<rusqlite::Connection> = None;
    for event in store.list_events(None, u32::MAX)? {
        if let Some(audio) = event.audio {
            let old = Path::new(&audio.path);
            let Some(parent) = old.parent() else {
                continue;
            };
            let final_path = parent.join(format!("{}.ogg", event.id));
            if old == final_path {
                continue;
            }
            if old.is_file() && !final_path.exists() {
                std::fs::rename(old, &final_path)?;
            }
            if final_path.is_file() {
                let connection = match &mut db {
                    Some(connection) => connection,
                    slot @ None => slot.insert(rusqlite::Connection::open(
                        store.data_dir().join("tuldmi.db"),
                    )?),
                };
                let relative = final_path
                    .strip_prefix(store.data_dir())
                    .map_err(|_| Error::Other("invalid audio path".into()))?
                    .to_string_lossy()
                    .replace('\\', "/");
                connection.execute(
                    "UPDATE audio_assets SET rel_path = ?1 WHERE id = ?2",
                    rusqlite::params![relative, audio.asset_id.to_string()],
                )?;
            }
        }
    }
    Ok(())
}
fn enabled(settings: &Settings, source: Source) -> bool {
    if source == Source::Mic {
        settings.mic_enabled
    } else {
        settings.system_enabled
    }
}
fn selected(settings: &Settings, source: Source) -> Option<&str> {
    if source == Source::Mic {
        settings.mic_device_id.as_deref()
    } else {
        settings.system_device_id.as_deref()
    }
}
pub(super) enum Command {
    Pause(bool, Sender<Result<()>>),
    Monitor(bool, Sender<Result<()>>),
    Settings(Settings, Sender<Result<()>>),
    Recover,
    Shutdown(Sender<Result<()>>),
}
pub(super) enum WriteJob {
    Segment {
        session: Uuid,
        source: Source,
        started_at: i64,
        ended_at: i64,
        samples: Vec<f32>,
    },
    End(Uuid, i64),
    Barrier(Sender<Result<()>>),
}
struct Pipeline {
    convert: ToMono16k,
    meter: LevelMeter,
    detector: Box<dyn SpeechDetector>,
    segmenter: Segmenter,
    pending: Vec<f32>,
    clock: Option<StreamClock>,
    total: u64,
    level: LevelFrame,
    drops: u64,
}
impl Pipeline {
    fn new(
        source: Source,
        rate: u32,
        channels: u16,
        settings: &Settings,
        model: Option<&Path>,
    ) -> Result<Self> {
        let v = &settings.vad;
        let segmenter = Segmenter::new(SegmenterConfig {
            threshold: v.threshold,
            min_speech_ms: v.min_speech_ms,
            pre_roll_ms: v.pre_roll_ms,
            post_roll_ms: v.post_roll_ms,
            min_silence_ms: v.min_silence_ms,
            max_segment_ms: v.max_segment_ms,
        });
        Ok(Self {
            convert: ToMono16k::new(rate, channels)?,
            meter: LevelMeter::new(),
            detector: best_detector(model),
            segmenter,
            pending: Vec::new(),
            clock: None,
            total: 0,
            level: LevelFrame {
                source,
                rms: 0.0,
                peak: 0.0,
                speaking: false,
            },
            drops: 0,
        })
    }
    fn samples(&mut self, samples: &[f32], recording: bool) -> Vec<SpeechSegment> {
        let mut readings = Vec::new();
        self.meter.push(samples, &mut readings);
        // Preserve peaks over the emission interval even for accelerated fake capture.
        for (rms, peak) in readings {
            self.level.rms = self.level.rms.max(rms);
            self.level.peak = self.level.peak.max(peak);
        }
        if !recording {
            return Vec::new();
        }
        self.pending.extend_from_slice(samples);
        let usable = self.pending.len() / FRAME_SAMPLES * FRAME_SAMPLES;
        let probabilities = self.detector.push(&self.pending[..usable]);
        let mut finished = Vec::new();
        for (frame, prob) in self.pending[..usable]
            .as_chunks::<FRAME_SAMPLES>()
            .0
            .iter()
            .zip(probabilities)
        {
            finished.extend(self.segmenter.push_frame(frame, prob));
        }
        self.pending.drain(..usable);
        self.level.speaking = self.segmenter.in_speech();
        finished
    }
}
struct Processor {
    shared: Arc<Shared>,
    streams: HashMap<Source, Box<dyn CaptureStream>>,
    pipelines: HashMap<Source, Pipeline>,
    tx: Sender<CaptureMsg>,
    rx: Receiver<CaptureMsg>,
    persist: Sender<WriteJob>,
    settings: Settings,
    session: Option<Uuid>,
    monitoring: bool,
    outbox: Mutex<VecDeque<WriteJob>>,
}
impl Processor {
    fn wants_devices(&self) -> bool {
        self.session.is_some() || self.monitoring
    }
    fn queue(&self, job: WriteJob) {
        let mut outbox = self.outbox.lock();
        if outbox.is_empty() {
            match self.persist.try_send(job) {
                Ok(()) => (),
                Err(TrySendError::Full(job)) => outbox.push_back(job),
                Err(TrySendError::Disconnected(_)) => self
                    .shared
                    .notice("audio writer unavailable; speech could not be saved"),
            }
        } else if outbox.len() < CAPTURE_CHANNEL_CAPACITY {
            outbox.push_back(job);
        } else {
            self.shared
                .notice("audio writer unavailable; speech queue exhausted");
        }
    }
    fn pump(&self) {
        let mut outbox = self.outbox.lock();
        while let Some(job) = outbox.pop_front() {
            match self.persist.try_send(job) {
                Ok(()) => (),
                Err(TrySendError::Full(job)) => {
                    outbox.push_front(job);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.shared.notice("audio writer stopped");
                    outbox.clear();
                    break;
                }
            }
        }
    }
    fn segments(&self, source: Source, pipeline: &Pipeline, segments: Vec<SpeechSegment>) {
        if let (Some(session), Some(clock)) = (self.session, &pipeline.clock) {
            for segment in segments {
                self.queue(WriteJob::Segment {
                    session,
                    source,
                    started_at: clock.unix_ms_of(segment.start_sample),
                    ended_at: clock.unix_ms_of(segment.end_sample()),
                    samples: segment.samples,
                });
            }
        }
    }
    fn flush_source(&mut self, source: Source) {
        if let Some(mut pipeline) = self.pipelines.remove(&source) {
            let mut tail = Vec::new();
            if pipeline.convert.flush(&mut tail).is_err() {
                self.shared
                    .notice("audio converter could not flush its tail");
            }
            let mut segments = pipeline.samples(&tail, self.session.is_some());
            if self.session.is_some() {
                if !pipeline.pending.is_empty() {
                    let mut frame = std::mem::take(&mut pipeline.pending);
                    frame.resize(FRAME_SAMPLES, 0.0);
                    let prob = pipeline
                        .detector
                        .push(&frame)
                        .first()
                        .copied()
                        .unwrap_or(0.0);
                    segments.extend(pipeline.segmenter.push_frame(&frame, prob));
                }
                if let Some(segment) = pipeline.segmenter.flush() {
                    segments.push(segment);
                }
            }
            self.segments(source, &pipeline, segments);
        }
    }
    fn stop_source(&mut self, source: Source) {
        if let Some(mut stream) = self.streams.remove(&source) {
            stream.stop();
        }
    }
    fn drain(&mut self) {
        for _ in 0..self.rx.len() {
            if let Ok(msg) = self.rx.try_recv() {
                self.message(msg);
            }
        }
    }
    fn close(&mut self) {
        for source in [Source::Mic, Source::System] {
            self.stop_source(source);
        }
        self.drain();
        for source in [Source::Mic, Source::System] {
            self.flush_source(source);
        }
        self.monitoring = false;
        if let Some(session) = self.session.take() {
            self.queue(WriteJob::End(
                session,
                self.shared.wall.to_unix_ms(Instant::now()),
            ));
        }
        let mut status = self.shared.status.lock();
        status.session_id = None;
        status.session_started_at = None;
        for source in &mut status.sources {
            source.state = SourceState::Disabled;
            source.format = None;
            source.error = None;
        }
    }
    fn open_source(&mut self, source: Source) {
        if !enabled(&self.settings, source) || self.shared.cancel_asr.load(Ordering::Acquire) {
            return;
        }
        match self
            .shared
            .provider
            .open(source, selected(&self.settings, source), self.tx.clone())
        {
            Ok(stream) => {
                self.streams.insert(source, stream);
            }
            Err(error) => self.failed(source, &error.to_string()),
        }
    }
    fn recompute_state(&self) {
        let mut status = self.shared.status.lock();
        if self.session.is_some() {
            status.state = if status
                .sources
                .iter()
                .any(|s| s.state == SourceState::Active)
            {
                CaptureState::Recording
            } else if status.sources.iter().any(|s| {
                s.enabled && matches!(s.state, SourceState::Unavailable | SourceState::Error)
            }) {
                CaptureState::Error
            } else {
                CaptureState::Recording
            };
        }
    }
    fn failed(&mut self, source: Source, error: &str) {
        self.stop_source(source);
        self.flush_source(source);
        {
            let mut status = self.shared.status.lock();
            if let Some(s) = status.sources.iter_mut().find(|s| s.source == source) {
                s.state = SourceState::Unavailable;
                s.error = Some(error.into());
            }
        }
        self.recompute_state();
        self.shared
            .notice(&format!("{} capture unavailable: {error}", source.as_str()));
        self.shared.publish();
    }
    fn message(&mut self, msg: CaptureMsg) {
        match msg {
            CaptureMsg::Opened {
                source,
                device_id,
                device_name,
                sample_rate,
                channels,
            } => {
                if !self.wants_devices()
                    || !enabled(&self.settings, source)
                    || !self.streams.contains_key(&source)
                {
                    return;
                }
                match Pipeline::new(
                    source,
                    sample_rate,
                    channels,
                    &self.settings,
                    self.shared.config.vad_model_path.as_deref(),
                ) {
                    Ok(pipeline) => {
                        if pipeline.detector.name() == "energy"
                            && self.shared.config.vad_model_path.is_some()
                        {
                            self.shared.notice("Silero unavailable; using energy VAD");
                        }
                        self.pipelines.insert(source, pipeline);
                    }
                    Err(error) => {
                        self.failed(source, &error.to_string());
                        return;
                    }
                }
                {
                    let mut status = self.shared.status.lock();
                    if let Some(s) = status.sources.iter_mut().find(|s| s.source == source) {
                        s.state = SourceState::Active;
                        s.device_id = Some(device_id);
                        s.device_name = Some(device_name);
                        s.format = Some(format!("{sample_rate} Hz, {channels} ch"));
                        s.error = None;
                    }
                }
                self.recompute_state();
                self.shared.publish();
            }
            CaptureMsg::Failed { source, error } => {
                if self.wants_devices() {
                    self.failed(source, &error);
                }
            }
            CaptureMsg::Audio { source, data, at } => {
                if let Some(mut pipeline) = self.pipelines.remove(&source) {
                    let mut samples = Vec::new();
                    if pipeline.convert.process(&data, &mut samples).is_err() {
                        self.failed(source, "audio conversion failed");
                        return;
                    }
                    if pipeline.clock.is_none() {
                        // The resampler may buffer its first native chunk. Anchor
                        // to the captured duration, including that buffered audio.
                        let first_samples = (data.len() as u64
                            / u64::from(pipeline.convert.channels()))
                        .saturating_mul(16_000)
                            / u64::from(pipeline.convert.in_rate());
                        pipeline.clock = Some(StreamClock::new(
                            self.shared.wall,
                            16_000,
                            at,
                            first_samples,
                        ));
                    }
                    pipeline.total += samples.len() as u64;
                    let segments = pipeline.samples(&samples, self.session.is_some());
                    // Timestamp complete segments before observing drift, preserving their duration.
                    self.segments(source, &pipeline, segments);
                    if let Some(clock) = &mut pipeline.clock {
                        clock.observe(pipeline.total, at);
                    }
                    self.pipelines.insert(source, pipeline);
                }
            }
        }
    }
    fn pause(&mut self, paused: bool, reply: Sender<Result<()>>) {
        if paused {
            self.close();
            self.shared.status.lock().state = CaptureState::Paused;
            self.queue(WriteJob::Barrier(reply));
        } else {
            if self.session.is_none() {
                self.close();
                match self
                    .shared
                    .store
                    .start_session(self.shared.wall.to_unix_ms(Instant::now()))
                {
                    Ok(session) => {
                        self.session = Some(session.id);
                        {
                            let mut status = self.shared.status.lock();
                            status.state = CaptureState::Starting;
                            status.session_id = Some(session.id);
                            status.session_started_at = Some(session.started_at);
                        }
                        // Device activation continues here after the caller sees Starting.
                        let _ = reply.try_send(Ok(()));
                        for source in [Source::Mic, Source::System] {
                            self.open_source(source);
                        }
                        self.drain();
                        self.recompute_state();
                    }
                    Err(error) => {
                        let _ = reply.try_send(Err(error));
                        return;
                    }
                }
            }
            let _ = reply.try_send(Ok(()));
        }
        self.shared.publish();
    }
    fn recover(&mut self) {
        if !self.wants_devices() {
            return;
        }
        for source in [Source::Mic, Source::System] {
            if !enabled(&self.settings, source) {
                continue;
            }
            let current = self
                .shared
                .status
                .lock()
                .sources
                .iter()
                .find(|s| s.source == source)
                .and_then(|s| s.device_id.clone());
            let changed = selected(&self.settings, source).is_none()
                && self.shared.provider.default_device_id(source) != current;
            if !self.streams.contains_key(&source) || changed {
                self.stop_source(source);
                self.drain();
                self.flush_source(source);
                self.open_source(source);
            }
        }
    }
}
pub(super) fn processing(
    shared: Arc<Shared>,
    commands: Receiver<Command>,
    persist: Sender<WriteJob>,
) {
    let (tx, rx) = bounded(CAPTURE_CHANNEL_CAPACITY);
    let mut p = Processor {
        settings: shared.settings.lock().clone(),
        shared: shared.clone(),
        streams: HashMap::new(),
        pipelines: HashMap::new(),
        tx,
        rx,
        persist,
        session: None,
        monitoring: false,
        outbox: Mutex::new(VecDeque::with_capacity(CAPTURE_CHANNEL_CAPACITY)),
    };
    if shared
        .config
        .vad_model_path
        .as_ref()
        .is_none_or(|path| !path.is_file())
    {
        shared.notice(
            "Silero model unavailable; energy VAD detects loud audio, not speech specifically",
        );
    }
    let tick = crossbeam_channel::tick(Duration::from_millis(33));
    let no_audio = crossbeam_channel::never();
    loop {
        if shared.quit.load(Ordering::Acquire) {
            p.close();
            break;
        }
        p.pump();
        // Keep control and level ticks responsive while codec/storage is behind.
        // The callback fills the bounded capture queue, then drops at its edge.
        let audio = if p.outbox.lock().is_empty() {
            p.rx.clone()
        } else {
            no_audio.clone()
        };
        crossbeam_channel::select! {
            recv(commands) -> command => match command {
                Ok(Command::Pause(paused, reply)) => p.pause(paused, reply),
                Ok(Command::Monitor(enabled, reply)) => {
                    if p.session.is_none() {
                        if enabled && !p.monitoring { p.monitoring = true; for source in [Source::Mic, Source::System] { p.open_source(source); } p.drain(); }
                        else if !enabled && p.monitoring { p.close(); }
                    }
                    let _ = reply.try_send(Ok(())); shared.publish();
                }
                Ok(Command::Settings(settings, reply)) => {
                    let old = std::mem::replace(&mut p.settings, settings.clone());
                    *shared.settings.lock() = settings.clone();
                    for source in [Source::Mic, Source::System] {
                        if enabled(&old, source) != enabled(&settings, source) || selected(&old, source) != selected(&settings, source) || old.vad != settings.vad {
                            p.stop_source(source); p.drain(); p.flush_source(source);
                            { let mut status = shared.status.lock(); if let Some(s) = status.sources.iter_mut().find(|s| s.source == source) { s.enabled = enabled(&settings, source); s.state = SourceState::Disabled; s.error = None; s.format = None; } }
                            if p.wants_devices() { p.open_source(source); }
                        }
                    }
                    p.drain(); p.recompute_state();
                    if old.retention != settings.retention { retain(&shared); }
                    let _ = reply.try_send(Ok(())); shared.publish();
                }
                Ok(Command::Recover) => p.recover(),
                Ok(Command::Shutdown(reply)) => {
                    p.close(); shared.status.lock().state = CaptureState::Stopped; shared.publish();
                    p.queue(WriteJob::Barrier(reply));
                    // Devices are closed. Drain durable work in order; the caller's
                    // shutdown deadline still bounds the join if storage stalls.
                    while let Some(job) = p.outbox.lock().pop_front() {
                        if p.persist.send(job).is_err() { break; }
                    }
                    break;
                }
                Err(_) => { p.close(); break; }
            },
            recv(audio) -> msg => if let Ok(msg) = msg { p.message(msg); },
            recv(tick) -> _ => {
                let mut frames = Vec::new();
                for (source, pipeline) in &mut p.pipelines {
                    frames.push(pipeline.level); pipeline.level.rms = 0.0; pipeline.level.peak = 0.0;
                    if let Some(stream) = p.streams.get(source) {
                        let count = stream.dropped_buffers();
                        if count > pipeline.drops { shared.notice(&format!("capture overloaded: {} buffers dropped for {}", count - pipeline.drops, source.as_str())); pipeline.drops = count; }
                    }
                }
                frames.sort_by_key(|f| if f.source == Source::Mic { 0 } else { 1 });
                if !frames.is_empty() { shared.emit(EngineEvent::Levels(frames)); }
                shared.publish();
            }
        }
    }
}

pub(super) fn writer(shared: Arc<Shared>, jobs: Receiver<WriteJob>) {
    while let Ok(job) = jobs.recv() {
        match job {
            WriteJob::Segment {
                session,
                source,
                started_at,
                ended_at,
                samples,
            } => {
                let result =
                    persist_segment(&shared, session, source, started_at, ended_at, &samples);
                match result {
                    Ok(_) => { shared.refresh_backlog(); let _ = shared.wake.try_send(()); }
                    Err(_) => shared.notice("could not encode or persist speech; check disk space and storage permissions"),
                }
            }
            WriteJob::End(session, ended_at) => {
                if shared.store.end_session(session, ended_at).is_err() {
                    shared.notice("could not end recording session");
                }
            }
            WriteJob::Barrier(reply) => {
                let _ = reply.try_send(Ok(()));
            }
        }
    }
}
fn persist_segment(
    shared: &Shared,
    session_id: Uuid,
    source: Source,
    started_at: i64,
    ended_at: i64,
    samples: &[f32],
) -> Result<SpeechEvent> {
    let date = chrono::DateTime::from_timestamp_millis(started_at)
        .ok_or_else(|| Error::Other("audio timestamp out of range".into()))?
        .with_timezone(&chrono::Local);
    let dir = shared
        .store
        .data_dir()
        .join("audio")
        .join(date.format("%Y/%m/%d").to_string());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.ogg", Uuid::now_v7()));
    let encoded = crate::encode::write_ogg_opus(&path, samples)?;
    let _gate = shared.persist_gate.lock();
    let inserted = shared.store.insert_event(&NewEvent {
        session_id,
        source,
        started_at,
        ended_at,
        audio: Some(NewAudio {
            path: path.clone(),
            sample_rate: 16_000,
            duration_ms: encoded.duration_ms,
            bytes: encoded.bytes as i64,
            peaks: encoded.peaks,
        }),
    });
    let mut event = match inserted {
        Ok(event) => event,
        Err(error) => {
            let _ = unlink(&path);
            return Err(error);
        }
    };
    // Store allocates event IDs internally. Finalize the filename after insertion,
    // before announcing/waking ASR. Restore the old path if finalization fails.
    let final_path = dir.join(format!("{}.ogg", event.id));
    if let Some(audio) = &mut event.audio {
        let finalized = (|| -> Result<()> {
            let mut db = rusqlite::Connection::open(shared.store.data_dir().join("tuldmi.db"))?;
            db.busy_timeout(Duration::from_secs(2))?;
            let transaction =
                db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let relative = final_path
                .strip_prefix(shared.store.data_dir())
                .map_err(|_| Error::Other("invalid audio path".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            transaction.execute(
                "UPDATE audio_assets SET rel_path = ?1 WHERE id = ?2",
                rusqlite::params![relative, audio.asset_id.to_string()],
            )?;
            std::fs::rename(&path, &final_path)?;
            if let Err(error) = transaction.commit() {
                let _ = std::fs::rename(&final_path, &path);
                return Err(error.into());
            }
            Ok(())
        })();
        if finalized.is_ok() {
            audio.path = final_path.to_string_lossy().into_owned();
        } else {
            shared.notice("could not finalize audio filename; original retained");
        }
    }
    shared.emit(EngineEvent::EventUpserted(event.clone()));
    Ok(event)
}
fn set_transcriber(shared: &Shared, state: TranscriberState, model: Option<&str>) {
    {
        let mut status = shared.status.lock();
        status.transcriber = state;
        status.model_id = model.map(str::to_owned);
    }
    shared.refresh_backlog();
}
pub(super) fn transcription(shared: Arc<Shared>, waking: Receiver<()>) {
    #[cfg(feature = "whisper")]
    {
        use crate::transcribe::{TranscriptionProvider, whisper::WhisperProvider};
        let mut provider: Option<WhisperProvider> = None;
        type ModelKey = (String, PathBuf, Option<std::time::SystemTime>);
        let mut loaded: Option<ModelKey> = None;
        let mut failed: Option<ModelKey> = None;
        while !shared.quit.load(Ordering::Acquire) && !shared.cancel_asr.load(Ordering::Acquire) {
            let settings = shared.settings.lock().clone();
            let installed = shared.store.installed_models().unwrap_or_else(|_| {
                shared.notice("could not read installed models");
                Vec::new()
            });
            let model = installed
                .into_iter()
                .find(|m| Some(&m.model_id) == settings.model_id.as_ref() && m.path.is_file());
            let Some(model) = model else {
                provider = None;
                loaded = None;
                failed = None;
                set_transcriber(&shared, TranscriberState::NoModel, None);
                let _ = waking.recv_timeout(Duration::from_millis(200));
                continue;
            };
            let key = (
                model.model_id.clone(),
                model.path.clone(),
                std::fs::metadata(&model.path)
                    .and_then(|m| m.modified())
                    .ok(),
            );
            if loaded.as_ref() != Some(&key) {
                if failed.as_ref() == Some(&key) {
                    let _ = waking.recv_timeout(Duration::from_secs(1));
                    continue;
                }
                provider = None;
                loaded = None;
                set_transcriber(&shared, TranscriberState::Loading, None);
                match WhisperProvider::load(&model.path, &model.model_id) {
                    Ok(mut p) => {
                        p.set_cancel(shared.cancel_asr.clone());
                        provider = Some(p);
                        loaded = Some(key.clone());
                        failed = None;
                    }
                    Err(_) => {
                        failed = Some(key);
                        set_transcriber(&shared, TranscriberState::Error, None);
                        shared.notice("selected transcription model could not load; reinstall it or choose another model");
                        continue;
                    }
                }
            }
            if shared.cancel_asr.load(Ordering::Acquire) {
                break;
            }
            if shared.settings.lock().model_id.as_ref() != Some(&model.model_id) {
                continue;
            }
            let next = {
                let _gate = shared.persist_gate.lock();
                shared.store.next_job()
            };
            let job = match next {
                Ok(Some(job)) => job,
                Ok(None) => {
                    set_transcriber(&shared, TranscriberState::Idle, Some(&model.model_id));
                    let _ = waking.recv_timeout(Duration::from_millis(200));
                    continue;
                }
                Err(_) => {
                    shared.notice("could not claim a transcription job");
                    let _ = waking.recv_timeout(Duration::from_secs(1));
                    continue;
                }
            };
            if shared.upsert(job.event_id).is_err() {
                shared.notice("could not announce transcription job");
            }
            set_transcriber(&shared, TranscriberState::Working, Some(&model.model_id));
            let result = (|| {
                let path = job
                    .audio_path
                    .as_ref()
                    .ok_or_else(|| Error::Transcription("audio is unavailable".into()))?;
                let samples = crate::encode::read_ogg_opus_16k(path)?;
                let provider = provider
                    .as_mut()
                    .ok_or_else(|| Error::Transcription("model is unavailable".into()))?;
                provider.transcribe(&samples, Some(&settings.language))
            })();
            // Never commit after shutdown: the running job is recovered on next startup.
            if shared.cancel_asr.load(Ordering::Acquire) {
                break;
            }
            let _gate = shared.persist_gate.lock();
            if shared.cancel_asr.load(Ordering::Acquire) {
                break;
            }
            match result {
                Ok(transcript) => {
                    match shared.store.save_transcript(
                        job.event_id,
                        &transcript.segments,
                        transcript.language.as_deref(),
                        &model.model_id,
                    ) {
                        Ok(event) => {
                            shared.emit(EngineEvent::EventUpserted(event));
                            if shared.settings.lock().retention
                                == Retention::DeleteAfterTranscription
                            {
                                match shared.store.expire_event_audio(job.event_id) {
                                    Ok(path) => {
                                        if let Some(path) = path
                                            && unlink(&path).is_err()
                                        {
                                            shared.notice("could not remove transcribed audio");
                                        }
                                        if shared.upsert(job.event_id).is_err() {
                                            shared.notice("could not announce audio expiration");
                                        }
                                    }
                                    Err(_) => shared.notice("could not expire transcribed audio"),
                                }
                            }
                        }
                        Err(_) => {
                            if shared
                                .store
                                .get_event(job.event_id)
                                .ok()
                                .flatten()
                                .is_some()
                            {
                                if shared
                                    .store
                                    .fail_job(job.event_id, "could not save transcript", true)
                                    .is_err()
                                {
                                    shared.notice("could not record transcript storage failure");
                                }
                                if shared.upsert(job.event_id).is_err() {
                                    shared.notice("could not announce transcription failure");
                                }
                                shared.notice("could not save transcript");
                            }
                        }
                    }
                }
                Err(_) => {
                    if shared
                        .store
                        .get_event(job.event_id)
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        if shared
                            .store
                            .fail_job(
                                job.event_id,
                                "transcription failed; check model and retained audio",
                                job.audio_path.is_some(),
                            )
                            .is_err()
                        {
                            shared.notice("could not record transcription failure");
                        }
                        let _ = shared.upsert(job.event_id);
                        shared.notice("transcription failed; retained audio can be retried");
                    }
                }
            }
            set_transcriber(&shared, TranscriberState::Idle, Some(&model.model_id));
        }
    }
    #[cfg(not(feature = "whisper"))]
    while !shared.quit.load(Ordering::Acquire) {
        set_transcriber(&shared, TranscriberState::NoModel, None);
        let _ = waking.recv_timeout(Duration::from_millis(200));
    }
}
fn retain(shared: &Shared) {
    let _gate = shared.persist_gate.lock();
    let policy = shared.settings.lock().retention;
    if policy == Retention::DeleteAfterTranscription {
        match shared.store.list_events(None, u32::MAX) {
            Ok(events) => {
                for event in events.into_iter().filter(|e| {
                    matches!(e.status, EventStatus::Done | EventStatus::Empty) && e.audio.is_some()
                }) {
                    match shared.store.expire_event_audio(event.id) {
                        Ok(path) => {
                            if let Some(path) = path
                                && unlink(&path).is_err()
                            {
                                shared.notice("could not remove transcribed audio");
                            }
                            if shared.upsert(event.id).is_err() {
                                shared.notice("could not announce audio expiration");
                            }
                        }
                        Err(_) => shared.notice("audio retention failed"),
                    }
                }
            }
            Err(_) => shared.notice("could not read events for retention"),
        }
        return;
    }
    if let Some(age) = policy.max_age_ms() {
        // Store protects queued and running audio from age expiration.
        let cutoff = chrono::Local::now().timestamp_millis().saturating_sub(age);
        let before = shared.store.list_events(None, u32::MAX).unwrap_or_default();
        match shared.store.expire_audio_before(cutoff) {
            Ok(paths) => {
                for path in &paths {
                    if unlink(path).is_err() {
                        shared.notice("could not remove expired audio");
                    }
                }
                for event in before {
                    if event
                        .audio
                        .as_ref()
                        .is_some_and(|a| paths.iter().any(|p| p == Path::new(&a.path)))
                    {
                        let _ = shared.upsert(event.id);
                    }
                }
            }
            Err(_) => shared.notice("audio retention failed"),
        }
    }
}
pub(super) fn housekeeping(shared: Arc<Shared>, commands: Sender<Command>) {
    retain(&shared);
    let mut last_retention = Instant::now();
    let mut last_recovery = Instant::now();
    while !shared.quit.load(Ordering::Acquire) {
        if last_recovery.elapsed() >= Duration::from_secs(3) {
            match commands.try_send(Command::Recover) {
                Ok(()) | Err(TrySendError::Full(_)) => (),
                Err(TrySendError::Disconnected(_)) => break,
            }
            last_recovery = Instant::now();
        }
        if last_retention.elapsed() >= Duration::from_secs(600) {
            retain(&shared);
            last_retention = Instant::now();
        }
        shared.publish();
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
