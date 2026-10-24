//! Domain and IPC types. This file is the contract shared with the frontend
//! (`src/lib/ipc/types.ts` mirrors it field-for-field). All serialize as camelCase.
//! Timestamps are wall-clock milliseconds since the Unix epoch (`i64`) unless named `*_ms`
//! relative offsets.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where audio came from. Tracked independently of who spoke it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Local microphone. Default speaker label: "Me".
    Mic,
    /// Render-endpoint loopback. Default speaker label: "Remote" (not diarized).
    System,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::System => "system",
        }
    }
    pub fn parse(s: &str) -> Option<Source> {
        match s {
            "mic" => Some(Source::Mic),
            "system" => Some(Source::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState {
    Stopped,
    Starting,
    Recording,
    Paused,
    /// Every enabled source failed. See per-source errors.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    Active,
    Disabled,
    /// Device missing or disconnected; engine retries periodically.
    Unavailable,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: Source,
    pub enabled: bool,
    pub state: SourceState,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    /// Native device format, e.g. "48000 Hz, 2 ch".
    pub format: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub session_id: Option<Uuid>,
    pub session_started_at: Option<i64>,
    pub sources: Vec<SourceStatus>,
    /// Speech events waiting for or undergoing transcription.
    pub backlog: u32,
    /// Id of the active transcription model, if one is installed and loaded.
    pub model_id: Option<String>,
    pub transcriber: TranscriberState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TranscriberState {
    /// No model installed/selected. Audio is still captured and queued.
    NoModel,
    Loading,
    Idle,
    Working,
    Error,
}

/// One meter reading. Emitted ~30 times/second per active source.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelFrame {
    pub source: Source,
    /// RMS over the window, linear 0..=1.
    pub rms: f32,
    /// Absolute peak over the window, linear 0..=1.
    pub peak: f32,
    /// VAD currently inside a speech region.
    pub speaking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Input,
    /// Output device usable for loopback capture.
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventStatus {
    /// Waiting in the transcription queue.
    Queued,
    Transcribing,
    Done,
    /// Transcribed, but no words recognized.
    Empty,
    Failed,
}

impl EventStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EventStatus::Queued => "queued",
            EventStatus::Transcribing => "transcribing",
            EventStatus::Done => "done",
            EventStatus::Empty => "empty",
            EventStatus::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Option<EventStatus> {
        Some(match s {
            "queued" => EventStatus::Queued,
            "transcribing" => EventStatus::Transcribing,
            "done" => EventStatus::Done,
            "empty" => EventStatus::Empty,
            "failed" => EventStatus::Failed,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    pub id: Uuid,
    pub label: String,
    /// "self" for the local user, "remote" for the undiarized system-audio speaker,
    /// "person" for future diarized/named speakers.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRef {
    pub asset_id: Uuid,
    /// Absolute path to the Ogg Opus file.
    pub path: String,
    pub duration_ms: i64,
    pub bytes: i64,
    /// Waveform envelope, `PEAKS_PER_SECOND` values per second, 0..=255.
    pub peaks: Vec<u8>,
}

pub const PEAKS_PER_SECOND: u32 = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: Uuid,
    pub event_id: Uuid,
    /// Offset from the event start.
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    /// 0..=1 when the backend reports it.
    pub confidence: Option<f32>,
}

/// One VAD-delimited span of speech from one source. The timeline's unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEvent {
    pub id: Uuid,
    pub session_id: Uuid,
    pub source: Source,
    pub speaker: Option<Speaker>,
    pub started_at: i64,
    pub ended_at: i64,
    pub status: EventStatus,
    /// None when audio was never kept or has expired under retention.
    pub audio: Option<AudioRef>,
    /// True if audio existed and was removed by retention or the user.
    pub audio_expired: bool,
    pub segments: Vec<TranscriptSegment>,
    pub language: Option<String>,
    pub model_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: Uuid,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    /// Empty = all sources.
    #[serde(default)]
    pub sources: Vec<Source>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<u32>,
}

/// Highlight delimiters inside `SearchHit::snippet`. Control characters so they can't
/// collide with transcript text; the frontend splits on them (never renders as HTML).
pub const HIGHLIGHT_START: char = '\u{2}';
pub const HIGHLIGHT_END: char = '\u{3}';

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub event_id: Uuid,
    pub segment_id: Uuid,
    pub source: Source,
    /// Wall-clock time of the matching segment (event start + segment offset).
    pub at: i64,
    pub snippet: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Retention {
    DeleteAfterTranscription,
    Days7,
    Days30,
    Forever,
}

impl Retention {
    /// Max age of retained audio, None = keep forever. `DeleteAfterTranscription` is handled
    /// at transcription time, not by age.
    pub fn max_age_ms(self) -> Option<i64> {
        const DAY: i64 = 86_400_000;
        match self {
            Retention::DeleteAfterTranscription => Some(0),
            Retention::Days7 => Some(7 * DAY),
            Retention::Days30 => Some(30 * DAY),
            Retention::Forever => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VadSettings {
    /// Speech probability to enter speech, 0..=1.
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub pre_roll_ms: u32,
    pub post_roll_ms: u32,
    pub min_silence_ms: u32,
    pub max_segment_ms: u32,
}

impl Default for VadSettings {
    fn default() -> Self {
        VadSettings {
            threshold: 0.5,
            min_speech_ms: 250,
            pre_roll_ms: 300,
            post_roll_ms: 700,
            min_silence_ms: 700,
            max_segment_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub onboarding_complete: bool,
    pub mic_enabled: bool,
    /// None = follow the system default device.
    pub mic_device_id: Option<String>,
    pub system_enabled: bool,
    pub system_device_id: Option<String>,
    /// Start capturing automatically when the app starts.
    pub capture_on_launch: bool,
    pub vad: VadSettings,
    pub retention: Retention,
    pub model_id: Option<String>,
    /// "auto" or an ISO-639-1 code.
    pub language: String,
    pub launch_at_login: bool,
    /// Tauri accelerator string.
    pub pause_shortcut: String,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            onboarding_complete: false,
            mic_enabled: true,
            mic_device_id: None,
            system_enabled: true,
            system_device_id: None,
            capture_on_launch: true,
            vad: VadSettings::default(),
            retention: Retention::Days30,
            model_id: None,
            language: "auto".into(),
            launch_at_login: false,
            pause_shortcut: "CommandOrControl+Shift+Backquote".into(),
            theme: Theme::System,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    /// "English" or "Multilingual".
    pub languages: String,
    pub license: String,
    pub url: String,
    pub installed: bool,
    pub recommended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Downloading,
    Verifying,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    pub model_id: String,
    pub state: DownloadState,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub data_dir: String,
    pub database_bytes: u64,
    pub audio_bytes: u64,
    pub audio_files: u64,
    pub models_bytes: u64,
    pub events: u64,
    pub transcribed_events: u64,
    pub free_disk_bytes: Option<u64>,
}

/// Everything the engine pushes to the UI. The Tauri layer maps each variant to an event
/// name (see `src-tauri/src/events.rs`).
#[allow(clippy::large_enum_variant)] // cloned rarely; boxing would complicate every match site
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "payload")]
pub enum EngineEvent {
    Status(CaptureStatus),
    Levels(Vec<LevelFrame>),
    EventUpserted(SpeechEvent),
    EventDeleted(Uuid),
    ModelProgress(ModelProgress),
    /// A non-fatal problem the user should see (device lost, job failed, disk low...).
    Notice { level: NoticeLevel, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}
