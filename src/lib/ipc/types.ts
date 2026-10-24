// Mirror of crates/engine/src/types.rs. Field order is kept identical so drift is diffable.
// All timestamps are wall-clock ms since the Unix epoch unless named `*Ms` (relative offsets).
// Uuid serializes as a string.

export type Uuid = string;

/** Where audio came from. Tracked independently of who spoke it. */
export type Source = "mic" | "system";

export type CaptureState = "stopped" | "starting" | "recording" | "paused" | "error";

export type SourceState = "active" | "disabled" | "unavailable" | "error";

export interface SourceStatus {
  source: Source;
  enabled: boolean;
  state: SourceState;
  deviceId: string | null;
  deviceName: string | null;
  /** Native device format, e.g. "48000 Hz, 2 ch". */
  format: string | null;
  error: string | null;
}

export interface CaptureStatus {
  state: CaptureState;
  sessionId: Uuid | null;
  sessionStartedAt: number | null;
  sources: SourceStatus[];
  /** Speech events waiting for or undergoing transcription. */
  backlog: number;
  /** Id of the active transcription model, if one is installed and loaded. */
  modelId: string | null;
  transcriber: TranscriberState;
}

export type TranscriberState = "noModel" | "loading" | "idle" | "working" | "error";

/** One meter reading. Emitted ~30 times/second per active source. */
export interface LevelFrame {
  source: Source;
  /** RMS over the window, linear 0..=1. */
  rms: number;
  /** Absolute peak over the window, linear 0..=1. */
  peak: number;
  /** VAD currently inside a speech region. */
  speaking: boolean;
}

export type DeviceKind = "input" | "output";

export interface DeviceInfo {
  id: string;
  name: string;
  kind: DeviceKind;
  isDefault: boolean;
}

export type EventStatus = "queued" | "transcribing" | "done" | "empty" | "failed";

export interface Speaker {
  id: Uuid;
  label: string;
  /** "self" | "remote" | "person" */
  kind: string;
}

export interface AudioRef {
  assetId: Uuid;
  /** Absolute path to the Ogg Opus file. */
  path: string;
  durationMs: number;
  bytes: number;
  /** Waveform envelope, PEAKS_PER_SECOND values per second, 0..=255. */
  peaks: number[];
}

export const PEAKS_PER_SECOND = 50;

export interface TranscriptSegment {
  id: Uuid;
  eventId: Uuid;
  /** Offset from the event start. */
  startMs: number;
  endMs: number;
  text: string;
  /** 0..=1 when the backend reports it. */
  confidence: number | null;
}

/** One VAD-delimited span of speech from one source. The timeline's unit. */
export interface SpeechEvent {
  id: Uuid;
  sessionId: Uuid;
  source: Source;
  speaker: Speaker | null;
  startedAt: number;
  endedAt: number;
  status: EventStatus;
  /** null when audio was never kept or has expired under retention. */
  audio: AudioRef | null;
  /** True if audio existed and was removed by retention or the user. */
  audioExpired: boolean;
  segments: TranscriptSegment[];
  language: string | null;
  modelId: string | null;
  error: string | null;
}

export interface Session {
  id: Uuid;
  startedAt: number;
  endedAt: number | null;
}

export interface SearchQuery {
  text: string;
  /** Empty = all sources. */
  sources: Source[];
  from: number | null;
  to: number | null;
  limit: number | null;
}

/** Highlight delimiters inside SearchHit.snippet. Split on them; never render as HTML. */
export const HIGHLIGHT_START = "\u0002";
export const HIGHLIGHT_END = "\u0003";

export interface SearchHit {
  eventId: Uuid;
  segmentId: Uuid;
  source: Source;
  /** Wall-clock time of the matching segment (event start + segment offset). */
  at: number;
  snippet: string;
  rank: number;
}

export type Retention = "deleteAfterTranscription" | "days7" | "days30" | "forever";

export type Theme = "system" | "light" | "dark";

export interface VadSettings {
  /** Speech probability to enter speech, 0..=1. */
  threshold: number;
  minSpeechMs: number;
  preRollMs: number;
  postRollMs: number;
  minSilenceMs: number;
  maxSegmentMs: number;
}

export const DEFAULT_VAD: VadSettings = {
  threshold: 0.5,
  minSpeechMs: 250,
  preRollMs: 300,
  postRollMs: 700,
  minSilenceMs: 700,
  maxSegmentMs: 30_000,
};

export interface Settings {
  onboardingComplete: boolean;
  micEnabled: boolean;
  /** null = follow the system default device. */
  micDeviceId: string | null;
  systemEnabled: boolean;
  systemDeviceId: string | null;
  /** Start capturing automatically when the app starts. */
  captureOnLaunch: boolean;
  vad: VadSettings;
  retention: Retention;
  modelId: string | null;
  /** "auto" or an ISO-639-1 code. */
  language: string;
  launchAtLogin: boolean;
  /** Tauri accelerator string. */
  pauseShortcut: string;
  theme: Theme;
}

export const DEFAULT_SETTINGS: Settings = {
  onboardingComplete: false,
  micEnabled: true,
  micDeviceId: null,
  systemEnabled: true,
  systemDeviceId: null,
  captureOnLaunch: true,
  vad: DEFAULT_VAD,
  retention: "days30",
  modelId: null,
  language: "auto",
  launchAtLogin: false,
  pauseShortcut: "CommandOrControl+Shift+Backquote",
  theme: "system",
};

export interface ModelInfo {
  id: string;
  name: string;
  description: string;
  sizeBytes: number;
  /** "English" or "Multilingual". */
  languages: string;
  license: string;
  url: string;
  installed: boolean;
  recommended: boolean;
}

export type DownloadState = "downloading" | "verifying" | "done" | "failed" | "cancelled";

export interface ModelProgress {
  modelId: string;
  state: DownloadState;
  downloaded: number;
  total: number;
  error: string | null;
}

export interface StorageStats {
  dataDir: string;
  databaseBytes: number;
  audioBytes: number;
  audioFiles: number;
  modelsBytes: number;
  events: number;
  transcribedEvents: number;
  freeDiskBytes: number | null;
}

// EngineEvent is not mirrored: the shell maps each variant to its own event name (docs/IPC.md).

export type NoticeLevel = "info" | "warning" | "error";

export interface Notice {
  level: NoticeLevel;
  message: string;
}

export type NavView = "timeline" | "settings" | "search";

export interface NavigateRequest {
  view: NavView;
  section?: string;
}

export type ExportFormat = "markdown" | "json";
