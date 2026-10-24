import type { Backend } from "../lib/ipc/backend";
import { errorMessage } from "../lib/ipc/backend";
import type {
  CaptureStatus,
  LevelFrame,
  ModelInfo,
  ModelProgress,
  Settings,
  Source,
  SpeechEvent,
} from "../lib/ipc/types";
import { mergeEvents } from "../lib/time";

type Listener = () => void;

/** Minimal observable value for useSyncExternalStore. */
export class Observable<T> {
  private listeners = new Set<Listener>();
  constructor(private value: T) {}
  get = () => this.value;
  set(next: T) {
    if (Object.is(next, this.value)) return;
    this.value = next;
    this.listeners.forEach((l) => l());
  }
  update(fn: (prev: T) => T) {
    this.set(fn(this.value));
  }
  subscribe = (l: Listener) => {
    this.listeners.add(l);
    return () => {
      this.listeners.delete(l);
    };
  };
}

/** Latest level per source, read by canvases every frame. Never drives React renders. */
export class LevelStore {
  private latest: Record<Source, LevelFrame & { at: number }> = {
    mic: { source: "mic", rms: 0, peak: 0, speaking: false, at: 0 },
    system: { source: "system", rms: 0, peak: 0, speaking: false, at: 0 },
  };
  /** Coarse, React-visible summary (changes rarely): which sources are live / speaking. */
  readonly summary = new Observable<Record<Source, { live: boolean; speaking: boolean }>>({
    mic: { live: false, speaking: false },
    system: { live: false, speaking: false },
  });

  private frameListeners = new Set<(frames: LevelFrame[]) => void>();

  push(frames: LevelFrame[]) {
    const now = performance.now();
    for (const f of frames) this.latest[f.source] = { ...f, at: now };
    this.frameListeners.forEach((l) => l(frames));
    this.refreshSummary();
  }

  /** Raw frame stream (for meters keeping their own history). */
  onFrames(fn: (frames: LevelFrame[]) => void): () => void {
    this.frameListeners.add(fn);
    return () => {
      this.frameListeners.delete(fn);
    };
  }

  get(source: Source) {
    return this.latest[source];
  }

  /** True when a frame arrived recently (levels stream is live for this source). */
  isLive(source: Source, now = performance.now()) {
    const at = this.latest[source].at;
    return at > 0 && now - at < 600;
  }

  refreshSummary() {
    const now = performance.now();
    const s = this.summary.get();
    const next = {
      mic: { live: this.isLive("mic", now), speaking: this.isLive("mic", now) && this.latest.mic.speaking },
      system: { live: this.isLive("system", now), speaking: this.isLive("system", now) && this.latest.system.speaking },
    };
    if (
      s.mic.live !== next.mic.live ||
      s.mic.speaking !== next.mic.speaking ||
      s.system.live !== next.system.live ||
      s.system.speaking !== next.system.speaking
    ) {
      this.summary.set(next);
    }
  }
}

export interface TimelineState {
  events: SpeechEvent[];
  loading: boolean;
  /** More history exists before the oldest loaded event. */
  hasMore: boolean;
  error: string | null;
  /** Increments for each live arrival (new id) so the view can count unseen events. */
  arrivals: { id: string; seq: number }[];
}

const PAGE = 100;

export class AppStore {
  readonly status = new Observable<CaptureStatus | null>(null);
  readonly settings = new Observable<Settings | null>(null);
  readonly models = new Observable<ModelInfo[] | null>(null);
  readonly progress = new Observable<Record<string, ModelProgress>>({});
  readonly levels = new LevelStore();
  readonly timeline = new Observable<TimelineState>({
    events: [],
    loading: true,
    hasMore: true,
    error: null,
    arrivals: [],
  });
  readonly bootError = new Observable<string | null>(null);

  private unsubs: (() => void)[] = [];
  private seq = 0;
  private summaryTimer: ReturnType<typeof setInterval> | null = null;

  constructor(readonly backend: Backend) {}

  async start() {
    const b = this.backend;
    this.unsubs.push(
      b.on("tuldmi://status", (s) => this.status.set(s)),
      b.on("tuldmi://levels", (f) => this.levels.push(f)),
      b.on("tuldmi://event-upserted", (ev) => this.onUpsert(ev)),
      b.on("tuldmi://event-deleted", (id) =>
        this.timeline.update((t) => ({ ...t, events: t.events.filter((e) => e.id !== id) })),
      ),
      b.on("tuldmi://model-progress", (p) => this.onProgress(p)),
    );
    // Levels can stop without a final frame; decay the summary so "speaking" doesn't stick.
    this.summaryTimer = setInterval(() => this.levels.refreshSummary(), 500);
    try {
      const [status, settings] = await Promise.all([b.getStatus(), b.getSettings()]);
      this.status.set(status);
      this.settings.set(settings);
    } catch (e) {
      this.bootError.set(errorMessage(e));
      return;
    }
    void this.refreshModels();
    void this.loadInitial();
  }

  stop() {
    this.unsubs.forEach((u) => u());
    this.unsubs = [];
    if (this.summaryTimer) clearInterval(this.summaryTimer);
  }

  // ---- timeline ----------------------------------------------------------------------------

  async loadInitial() {
    this.timeline.update((t) => ({ ...t, loading: true, error: null }));
    try {
      const page = await this.backend.listEvents(null, PAGE);
      this.timeline.update((t) => ({
        ...t,
        events: mergeEvents(t.events, page),
        loading: false,
        hasMore: page.length >= PAGE,
      }));
    } catch (e) {
      this.timeline.update((t) => ({ ...t, loading: false, error: errorMessage(e) }));
    }
  }

  private loadingOlder = false;
  async loadOlder() {
    const t = this.timeline.get();
    if (this.loadingOlder || !t.hasMore || t.events.length === 0) return;
    this.loadingOlder = true;
    this.timeline.set({ ...t, loading: true });
    try {
      const oldest = t.events[0].startedAt;
      const page = await this.backend.listEvents(oldest, PAGE);
      this.timeline.update((cur) => ({
        ...cur,
        events: mergeEvents(cur.events, page),
        loading: false,
        hasMore: page.length >= PAGE,
      }));
    } catch (e) {
      this.timeline.update((cur) => ({ ...cur, loading: false, error: errorMessage(e) }));
    } finally {
      this.loadingOlder = false;
    }
  }

  /** Ensure an event (e.g. a search hit) is loaded, paging back until it is. */
  async ensureLoaded(id: string): Promise<SpeechEvent | null> {
    const have = this.timeline.get().events.find((e) => e.id === id);
    if (have) return have;
    const ev = await this.backend.getEvent(id);
    if (!ev) return null;
    // Load everything between the target and what we have, so the timeline stays contiguous.
    let guard = 0;
    while (guard++ < 50) {
      const t = this.timeline.get();
      if (!t.hasMore || t.events.length === 0 || t.events[0].startedAt <= ev.startedAt) break;
      await this.loadOlder();
    }
    this.timeline.update((t) => ({ ...t, events: mergeEvents(t.events, [ev]) }));
    return ev;
  }

  private onUpsert(ev: SpeechEvent) {
    this.timeline.update((t) => {
      const isNew = !t.events.some((e) => e.id === ev.id);
      return {
        ...t,
        events: mergeEvents(t.events, [ev]),
        arrivals: isNew ? [...t.arrivals.slice(-200), { id: ev.id, seq: ++this.seq }] : t.arrivals,
      };
    });
  }

  removeLocal(id: string) {
    this.timeline.update((t) => ({ ...t, events: t.events.filter((e) => e.id !== id) }));
  }

  // ---- settings & models -------------------------------------------------------------------

  async saveSettings(next: Settings): Promise<Settings> {
    const prev = this.settings.get();
    this.settings.set(next); // optimistic
    try {
      const saved = await this.backend.updateSettings(next);
      this.settings.set(saved);
      return saved;
    } catch (e) {
      this.settings.set(prev);
      throw e;
    }
  }

  async refreshModels() {
    try {
      this.models.set(await this.backend.listModels());
    } catch {
      this.models.set([]);
    }
  }

  private onProgress(p: ModelProgress) {
    this.progress.update((all) => ({ ...all, [p.modelId]: p }));
    if (p.state === "done") {
      void this.refreshModels();
      // Downloading a model with none selected means "use this one".
      const s = this.settings.get();
      if (s && !s.modelId) void this.saveSettings({ ...s, modelId: p.modelId }).catch(() => {});
    }
  }

  clearProgress(modelId: string) {
    this.progress.update((all) => {
      const next = { ...all };
      delete next[modelId];
      return next;
    });
  }
}
