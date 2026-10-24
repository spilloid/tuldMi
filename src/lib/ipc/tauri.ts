import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend, BackendEventName, BackendEvents, EventCursor, Unsubscribe } from "./backend";
import type {
  CaptureStatus,
  DeviceInfo,
  ExportFormat,
  ModelInfo,
  SearchHit,
  SearchQuery,
  Settings,
  SpeechEvent,
  StorageStats,
} from "./types";

/** Real backend: Tauri `invoke` commands and `listen` events (docs/IPC.md). */
export class TauriBackend implements Backend {
  readonly kind = "tauri" as const;

  getStatus = () => invoke<CaptureStatus>("get_status");
  setPaused = (paused: boolean) => invoke<CaptureStatus>("set_paused", { paused });
  setMonitoring = (enabled: boolean) => invoke<CaptureStatus>("set_monitoring", { enabled });
  listDevices = () => invoke<DeviceInfo[]>("list_devices");
  getSettings = () => invoke<Settings>("get_settings");
  updateSettings = (settings: Settings) => invoke<Settings>("update_settings", { settings });
  listEvents = (cursor: EventCursor | null, limit: number) =>
    invoke<SpeechEvent[]>("list_events", { before: cursor?.before, beforeId: cursor?.beforeId, limit });
  getEvent = (id: string) => invoke<SpeechEvent | null>("get_event", { id });
  search = (query: SearchQuery) => invoke<SearchHit[]>("search", { query });
  retryEvent = (id: string) => invoke<boolean>("retry_event", { id });
  deleteEvent = (id: string) => invoke<void>("delete_event", { id });
  deleteEventAudio = (id: string) => invoke<void>("delete_event_audio", { id });
  deleteAllHistory = () => invoke<void>("delete_all_history");
  listModels = () => invoke<ModelInfo[]>("list_models");
  downloadModel = (modelId: string) => invoke<void>("download_model", { modelId });
  cancelModelDownload = (modelId: string) => invoke<void>("cancel_model_download", { modelId });
  deleteModel = (modelId: string) => invoke<void>("delete_model", { modelId });
  storageStats = () => invoke<StorageStats>("storage_stats");
  openDataDir = () => invoke<void>("open_data_dir");
  exportTranscripts = (format: ExportFormat, from: number | null, to: number | null) =>
    invoke<string>("export_transcripts", { format, from: from ?? undefined, to: to ?? undefined });
  revealPath = (path: string) => invoke<void>("reveal_path", { path });
  quitApp = () => invoke<void>("quit_app");

  audioUrl(path: string) {
    return convertFileSrc(path);
  }

  on<E extends BackendEventName>(event: E, handler: (payload: BackendEvents[E]) => void): Unsubscribe {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<BackendEvents[E]>(event, (e) => handler(e.payload)).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }
}
