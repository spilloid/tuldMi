import type {
  CaptureStatus,
  DeviceInfo,
  ExportFormat,
  LevelFrame,
  ModelInfo,
  ModelProgress,
  NavigateRequest,
  Notice,
  SearchHit,
  SearchQuery,
  Settings,
  SpeechEvent,
  StorageStats,
} from "./types";

/** Event name -> payload, per docs/IPC.md. */
export interface BackendEvents {
  "tuldmi://status": CaptureStatus;
  "tuldmi://levels": LevelFrame[];
  "tuldmi://event-upserted": SpeechEvent;
  "tuldmi://event-deleted": string;
  "tuldmi://model-progress": ModelProgress;
  "tuldmi://notice": Notice;
  "tuldmi://navigate": NavigateRequest;
}

export type BackendEventName = keyof BackendEvents;

/** Keyset-pagination cursor for list_events. */
export interface EventCursor {
  before: number;
  beforeId: string;
}

/** Strict (startedAt, id) ordering shared by the demo backend, merging, and cursors. */
export function compareEvents(a: { startedAt: number; id: string }, b: { startedAt: number; id: string }): number {
  return a.startedAt - b.startedAt || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}
export type Unsubscribe = () => void;

/** Every command in docs/IPC.md. All reject with a user-presentable string. */
export interface Backend {
  readonly kind: "tauri" | "demo";

  getStatus(): Promise<CaptureStatus>;
  setPaused(paused: boolean): Promise<CaptureStatus>;
  setMonitoring(enabled: boolean): Promise<CaptureStatus>;
  listDevices(): Promise<DeviceInfo[]>;
  getSettings(): Promise<Settings>;
  updateSettings(settings: Settings): Promise<Settings>;
  /** Newest first, ordered by (startedAt desc, id desc). The cursor is the oldest event
   *  already held: results are strictly older than (before, beforeId), so events sharing a
   *  start time are never skipped. */
  listEvents(cursor: EventCursor | null, limit: number): Promise<SpeechEvent[]>;
  getEvent(id: string): Promise<SpeechEvent | null>;
  search(query: SearchQuery): Promise<SearchHit[]>;
  retryEvent(id: string): Promise<boolean>;
  deleteEvent(id: string): Promise<void>;
  deleteEventAudio(id: string): Promise<void>;
  deleteAllHistory(): Promise<void>;
  listModels(): Promise<ModelInfo[]>;
  downloadModel(modelId: string): Promise<void>;
  cancelModelDownload(modelId: string): Promise<void>;
  deleteModel(modelId: string): Promise<void>;
  storageStats(): Promise<StorageStats>;
  openDataDir(): Promise<void>;
  exportTranscripts(format: ExportFormat, from: number | null, to: number | null): Promise<string>;
  revealPath(path: string): Promise<void>;
  quitApp(): Promise<void>;

  /** Playable URL for a retained audio file, or null if this backend can't play audio. */
  audioUrl(path: string): string | null;

  on<E extends BackendEventName>(event: E, handler: (payload: BackendEvents[E]) => void): Unsubscribe;
}

/** Normalise whatever a command rejected with into a presentable message. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return "Something went wrong.";
  }
}
