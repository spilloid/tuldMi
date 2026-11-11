import type { Backend } from "../lib/ipc/backend";
import { compareEvents, errorMessage } from "../lib/ipc/backend";
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
      b.on("tuldmi://event-deleted", (id) => {
        this.held.delete(id);
        this.timeline.update((t) => ({ ...t, events: t.events.filter((e) => e.id !== id) }));
      }),
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

  /** The first page, once asked for: navigation waits for it, so it never races the page. */
  private initial: Promise<void> | null = null;
  loadInitial(): Promise<void> {
    this.initial = this.fetchInitial();
    return this.initial;
  }
  private async fetchInitial() {
    this.timeline.update((t) => ({ ...t, loading: true, error: null }));
    try {
      const page = await this.backend.listEvents(null, PAGE);
      this.paged(page);
      this.timeline.update((t) => ({
        ...t,
        events: this.release(mergeEvents(t.events, page)),
        loading: false,
      }));
    } catch (e) {
      this.timeline.update((t) => ({ ...t, loading: false, error: errorMessage(e) }));
    }
  }

  /** The oldest event of the pages loaded so far: where the next page starts. Kept apart from
   *  the list, which live updates and navigation also add to, so paging never skips history. */
  private boundary: Pick<SpeechEvent, "startedAt" | "id"> | null = null;
  /** Whether history continues below `boundary` (mirrored in the timeline state). */
  private more = true;
  /** Updates to history below the pages loaded so far (speaker backfill, Transcribe anyway, a
   *  retry, a late-stored clip), newest version per id. Shown as soon as their page is in, so
   *  the list stays contiguous and a range or conversation copy never silently misses events. */
  private held = new Map<string, SpeechEvent>();

  private paged(page: SpeechEvent[]) {
    if (page.length > 0) this.boundary = page[page.length - 1];
    this.more = page.length >= PAGE;
    this.timeline.update((t) => ({ ...t, hasMore: this.more }));
  }

  /** `events` plus every held update now within the loaded pages (all of them at the end).
   *  Held versions came from notifications, so they are at least as new as a page's copy. */
  private release(events: SpeechEvent[]): SpeechEvent[] {
    const due: SpeechEvent[] = [];
    for (const [id, ev] of this.held) {
      if (!this.more || (this.boundary && compareEvents(ev, this.boundary) >= 0)) {
        due.push(ev);
        this.held.delete(id);
      }
    }
    return due.length ? mergeEvents(events, due) : events;
  }

  private older: Promise<void> | null = null;
  /** Load the next page of history. Concurrent calls share the one in flight. */
  loadOlder(): Promise<void> {
    if (this.older) return this.older;
    const oldest = this.boundary;
    if (!this.more || !oldest) return Promise.resolve();
    this.older = (async () => {
      this.timeline.update((t) => ({ ...t, loading: true }));
      try {
        const page = await this.backend.listEvents({ before: oldest.startedAt, beforeId: oldest.id }, PAGE);
        this.paged(page);
        this.timeline.update((cur) => ({
          ...cur,
          events: this.release(mergeEvents(cur.events, page)),
          loading: false,
        }));
      } catch (e) {
        // The boundary stays put, which is how a caller paging in a loop sees it failed.
        this.timeline.update((cur) => ({ ...cur, loading: false, error: errorMessage(e) }));
      } finally {
        this.older = null;
      }
    })();
    return this.older;
  }

  /** Ensure an event (e.g. a search hit) is loaded, paging back until it is. */
  async ensureLoaded(id: string): Promise<SpeechEvent | null> {
    if (this.initial) await this.initial;
    const have = this.timeline.get().events.find((e) => e.id === id);
    if (have) return have;
    const ev = await this.backend.getEvent(id);
    if (!ev) return null;
    // Load everything between the target and what we have, so the timeline stays contiguous
    // (every page, however far back: a range copy must never have a hole in it).
    while (this.more && this.boundary && compareEvents(this.boundary, ev) > 0) {
      const before = this.boundary;
      await this.loadOlder();
      if (this.boundary === before) break;
    }
    if (this.more && this.boundary && compareEvents(ev, this.boundary) < 0) {
      // Couldn't reach it (a page failed): show it once its page is in, not as an island.
      this.held.set(ev.id, ev);
      return null;
    }
    this.timeline.update((t) => ({ ...t, events: mergeEvents(t.events, [ev]) }));
    return this.timeline.get().events.find((e) => e.id === ev.id) ?? ev;
  }

  private onUpsert(ev: SpeechEvent) {
    // Below the loaded pages (or before the first page is in): hold it until its page is.
    if (this.more && (!this.boundary || compareEvents(ev, this.boundary) < 0) && !this.timeline.get().events.some((e) => e.id === ev.id)) {
      // (Before the first page this includes new live speech; that page releases it at once.)
      this.held.set(ev.id, ev);
      return;
    }
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
      // Downloading a transcription model with none selected means "use this one" (speaker
      // models download through the same events but are never a transcription model).
      const s = this.settings.get();
      const transcription = this.models.get()?.some((m) => m.id === p.modelId);
      if (s && !s.modelId && transcription) void this.saveSettings({ ...s, modelId: p.modelId }).catch(() => {});
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
