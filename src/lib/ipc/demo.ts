import type { Backend, BackendEventName, BackendEvents, Unsubscribe } from "./backend";
import {
  DEMO_DEVICES,
  DEMO_MODEL_ID,
  DEMO_MODELS,
  LIVE_SCRIPT,
  buildDemoEvents,
  makeEvent,
} from "./demoData";
import { searchEvents } from "./demoSearch";
import { DEFAULT_SETTINGS } from "./types";
import type {
  CaptureStatus,
  DeviceInfo,
  ExportFormat,
  ModelInfo,
  ModelProgress,
  SearchHit,
  SearchQuery,
  Settings,
  SpeechEvent,
  StorageStats,
} from "./types";

export interface DemoOptions {
  /** Start with onboarding not completed. */
  fresh?: boolean;
  /** Start with no history at all (empty-state preview). */
  empty?: boolean;
  /** Disable the scripted live arrivals (for deterministic screenshots). */
  still?: boolean;
  now?: number;
}

const DEMO_DATA_DIR = "C:\\Users\\you\\AppData\\Roaming\\tuldMi (demo — nothing is stored)";

/**
 * In-memory backend with realistic sample data. Nothing touches disk or network.
 * Levels are never simulated (real signals only): no `tuldmi://levels` events are emitted.
 */
export class DemoBackend implements Backend {
  readonly kind = "demo" as const;

  private events: SpeechEvent[];
  private pendingText: Map<string, string[]>;
  private settings: Settings;
  private models: ModelInfo[] = DEMO_MODELS.map((m) => ({ ...m }));
  private downloads = new Map<string, ReturnType<typeof setInterval>>();
  private status: CaptureStatus;
  private listeners = new Map<string, Set<(p: unknown) => void>>();
  private timers: ReturnType<typeof setTimeout>[] = [];
  private liveIndex = 0;
  private monitoring = false;

  constructor(opts: DemoOptions = {}) {
    const now = opts.now ?? Date.now();
    const built = opts.empty ? { events: [], pendingText: new Map() } : buildDemoEvents(now);
    this.events = built.events;
    this.pendingText = built.pendingText;
    this.settings = {
      ...DEFAULT_SETTINGS,
      onboardingComplete: !opts.fresh,
      modelId: opts.fresh ? null : DEMO_MODEL_ID,
      retention: "days7",
    };
    if (opts.fresh) this.models = this.models.map((m) => ({ ...m, installed: false }));
    const sessionId = this.events[this.events.length - 1]?.sessionId ?? "d3m0d3m0-0000-4000-8000-5e5510000001";
    this.status = {
      state: opts.fresh ? "stopped" : "recording",
      sessionId: opts.fresh ? null : sessionId,
      sessionStartedAt: opts.fresh ? null : now - 26 * 60_000,
      sources: [
        { source: "mic", enabled: true, state: opts.fresh ? "disabled" : "active", deviceId: "in:usb-podcast", deviceName: "Shure MV7 (USB Audio)", format: "48000 Hz, 1 ch", error: null },
        { source: "system", enabled: true, state: opts.fresh ? "disabled" : "active", deviceId: "out:speakers", deviceName: "Speakers (Realtek Audio)", format: "48000 Hz, 2 ch", error: null },
      ],
      backlog: 0,
      modelId: opts.fresh ? null : DEMO_MODEL_ID,
      transcriber: opts.fresh ? "noModel" : "idle",
    };
    this.recomputeBacklog();
    if (!opts.fresh && !opts.still) this.startScript();
  }

  // ---- internals -------------------------------------------------------------------------

  private emit<E extends BackendEventName>(event: E, payload: BackendEvents[E]) {
    this.listeners.get(event)?.forEach((h) => h(structuredClone(payload)));
  }

  private later(ms: number, fn: () => void) {
    this.timers.push(setTimeout(fn, ms));
  }

  private recomputeBacklog() {
    const backlog = this.events.filter((e) => e.status === "queued" || e.status === "transcribing").length;
    const working = this.events.some((e) => e.status === "transcribing");
    this.status = {
      ...this.status,
      backlog,
      transcriber: this.status.modelId ? (working ? "working" : "idle") : "noModel",
    };
  }

  private pushStatus() {
    this.recomputeBacklog();
    this.emit("tuldmi://status", this.status);
  }

  private upsert(ev: SpeechEvent) {
    const i = this.events.findIndex((e) => e.id === ev.id);
    if (i >= 0) this.events[i] = ev;
    else this.events.push(ev);
    this.emit("tuldmi://event-upserted", ev);
    this.pushStatus();
  }

  /** Move queued/transcribing events along, as a real backlog would drain. */
  private transcribe(id: string, delay: number) {
    this.later(delay, () => {
      const ev = this.events.find((e) => e.id === id);
      if (!ev || !this.status.modelId) return;
      this.upsert({ ...ev, status: "transcribing" });
      this.later(1800 + (ev.endedAt - ev.startedAt) / 4, () => {
        const cur = this.events.find((e) => e.id === id);
        if (!cur) return;
        const texts = this.pendingText.get(id) ?? [];
        this.pendingText.delete(id);
        const done = makeEvent(cur.source, cur.sessionId, cur.startedAt, texts, texts.length ? "done" : "empty", null, false);
        this.upsert({ ...done, id: cur.id, segments: done.segments.map((s) => ({ ...s, eventId: cur.id })), endedAt: cur.endedAt });
      });
    });
  }

  private startScript() {
    let delay = 3500;
    for (const ev of this.events) {
      if (ev.status === "transcribing" || ev.status === "queued") {
        this.transcribe(ev.id, delay);
        delay += 4200;
      }
    }
    const tick = () => {
      if (this.liveIndex >= LIVE_SCRIPT.length) return;
      if (this.status.state === "recording") {
        const [source, text] = LIVE_SCRIPT[this.liveIndex++];
        const texts = Array.isArray(text) ? text : [text];
        const ev = makeEvent(source, this.status.sessionId!, Date.now() - 4000, texts, "queued", null, false);
        this.pendingText.set(ev.id, texts);
        this.upsert({ ...ev, endedAt: Date.now() - 400 });
        this.transcribe(ev.id, 1200);
      }
      this.later(16_000, tick);
    };
    this.later(14_000, tick);
  }

  dispose() {
    this.timers.forEach(clearTimeout);
    this.downloads.forEach(clearInterval);
  }

  // ---- commands ----------------------------------------------------------------------------

  async getStatus() {
    return structuredClone(this.status);
  }

  async setPaused(paused: boolean) {
    const now = Date.now();
    this.status = {
      ...this.status,
      state: paused ? "paused" : "recording",
      sessionId: paused ? this.status.sessionId : `d3m0d3m0-0000-4000-8000-5e55${String(now % 1e8).padStart(8, "0")}`,
      sessionStartedAt: paused ? this.status.sessionStartedAt : now,
      sources: this.status.sources.map((s) => ({
        ...s,
        state: paused || !s.enabled ? "disabled" : "active",
      })),
    };
    this.pushStatus();
    return structuredClone(this.status);
  }

  async setMonitoring(enabled: boolean) {
    // The demo has no audio devices, so monitoring opens nothing and no levels flow.
    this.monitoring = enabled;
    void this.monitoring;
    return structuredClone(this.status);
  }

  async listDevices(): Promise<DeviceInfo[]> {
    return structuredClone(DEMO_DEVICES);
  }

  async getSettings() {
    return structuredClone(this.settings);
  }

  async updateSettings(settings: Settings) {
    this.settings = structuredClone(settings);
    const findName = (id: string | null, kind: "input" | "output") =>
      (DEMO_DEVICES.find((d) => d.id === id) ?? DEMO_DEVICES.find((d) => d.kind === kind && d.isDefault))!;
    const mic = findName(settings.micDeviceId, "input");
    const sys = findName(settings.systemDeviceId, "output");
    const live = this.status.state === "recording";
    const modelInstalled = this.models.some((m) => m.id === settings.modelId && m.installed);
    this.status = {
      ...this.status,
      modelId: modelInstalled ? settings.modelId : null,
      sources: [
        { ...this.status.sources[0], enabled: settings.micEnabled, deviceId: mic.id, deviceName: mic.name, state: settings.micEnabled && live ? "active" : "disabled" },
        { ...this.status.sources[1], enabled: settings.systemEnabled, deviceId: sys.id, deviceName: sys.name, state: settings.systemEnabled && live ? "active" : "disabled" },
      ],
    };
    if (settings.onboardingComplete && this.status.state === "stopped" && settings.captureOnLaunch) {
      await this.setPaused(false);
    } else {
      this.pushStatus();
    }
    return structuredClone(this.settings);
  }

  async listEvents(before: number | null, limit: number) {
    const list = this.events
      .filter((e) => before == null || e.startedAt < before)
      .sort((a, b) => b.startedAt - a.startedAt)
      .slice(0, limit);
    return structuredClone(list);
  }

  async getEvent(id: string) {
    const e = this.events.find((x) => x.id === id);
    return e ? structuredClone(e) : null;
  }

  async search(query: SearchQuery): Promise<SearchHit[]> {
    return searchEvents(this.events, query);
  }

  async retryEvent(id: string) {
    const ev = this.events.find((e) => e.id === id);
    if (!ev || ev.status !== "failed") return false;
    this.upsert({ ...ev, status: "queued", error: null });
    this.transcribe(id, 600);
    return true;
  }

  async deleteEvent(id: string) {
    this.events = this.events.filter((e) => e.id !== id);
    this.emit("tuldmi://event-deleted", id);
    this.pushStatus();
  }

  async deleteEventAudio(id: string) {
    const ev = this.events.find((e) => e.id === id);
    if (ev) this.upsert({ ...ev, audio: null, audioExpired: true });
  }

  async deleteAllHistory() {
    const ids = this.events.map((e) => e.id);
    this.events = [];
    ids.forEach((id) => this.emit("tuldmi://event-deleted", id));
    this.pushStatus();
  }

  async listModels() {
    return structuredClone(this.models);
  }

  async downloadModel(modelId: string) {
    const m = this.models.find((x) => x.id === modelId);
    if (!m) throw "Unknown model.";
    if (this.downloads.has(modelId)) return;
    // Demo: no bytes are fetched. Progress is simulated and labelled as demo in the UI.
    let downloaded = 0;
    const total = m.sizeBytes;
    const step = total / 34;
    const iv = setInterval(() => {
      downloaded = Math.min(total, downloaded + step * (0.6 + ((downloaded / total) * 7) % 0.8));
      const p: ModelProgress = { modelId, state: "downloading", downloaded: Math.round(downloaded), total, error: null };
      if (downloaded >= total) {
        clearInterval(iv);
        this.downloads.delete(modelId);
        this.emit("tuldmi://model-progress", { ...p, state: "verifying" });
        this.later(700, () => {
          m.installed = true;
          this.emit("tuldmi://model-progress", { ...p, state: "done" });
        });
      } else this.emit("tuldmi://model-progress", p);
    }, 160);
    this.downloads.set(modelId, iv);
  }

  async cancelModelDownload(modelId: string) {
    const iv = this.downloads.get(modelId);
    if (iv) clearInterval(iv);
    this.downloads.delete(modelId);
    const m = this.models.find((x) => x.id === modelId);
    this.emit("tuldmi://model-progress", { modelId, state: "cancelled", downloaded: 0, total: m?.sizeBytes ?? 0, error: null });
  }

  async deleteModel(modelId: string) {
    const m = this.models.find((x) => x.id === modelId);
    if (m) m.installed = false;
    if (this.settings.modelId === modelId) {
      this.status = { ...this.status, modelId: null };
      this.pushStatus();
    }
  }

  async storageStats(): Promise<StorageStats> {
    const words = this.events.reduce((n, e) => n + e.segments.reduce((k, s) => k + s.text.length, 0), 0);
    return {
      dataDir: DEMO_DATA_DIR,
      databaseBytes: 1_420_000 + words * 6,
      audioBytes: 0,
      audioFiles: 0,
      modelsBytes: this.models.filter((m) => m.installed).reduce((n, m) => n + m.sizeBytes, 0),
      events: this.events.length,
      transcribedEvents: this.events.filter((e) => e.status === "done" || e.status === "empty").length,
      freeDiskBytes: 214_748_364_800,
    };
  }

  async openDataDir() {
    throw "Demo mode has no data folder — nothing is stored on disk.";
  }

  async exportTranscripts(_format: ExportFormat, _from: number | null, _to: number | null): Promise<string> {
    throw "Export is unavailable in demo mode — there are no real transcripts to write.";
  }

  async revealPath(_path: string) {
    throw "Demo mode has no files to reveal.";
  }

  async quitApp() {
    window.close();
  }

  audioUrl(_path: string) {
    return null;
  }

  on<E extends BackendEventName>(event: E, handler: (payload: BackendEvents[E]) => void): Unsubscribe {
    let set = this.listeners.get(event);
    if (!set) this.listeners.set(event, (set = new Set()));
    const h = handler as (p: unknown) => void;
    set.add(h);
    return () => set!.delete(h);
  }
}
