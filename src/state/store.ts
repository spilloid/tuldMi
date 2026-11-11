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
    // A fresh start (also after stop(), as React's strict mode does) re-reads the timeline from
    // scratch: anything kept from before may have missed updates while unsubscribed.
    this.resetTimeline();
    this.unsubs.push(
      b.on("tuldmi://status", (s) => this.status.set(s)),
      b.on("tuldmi://levels", (f) => this.levels.push(f)),
      b.on("tuldmi://event-upserted", (ev) => this.onUpsert(ev)),
      b.on("tuldmi://event-deleted", (id) => this.forget(id)),
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
    this.epoch++;
  }

  // ---- timeline ----------------------------------------------------------------------------

  /** Bumped by stop(): a page answered for an earlier start is dropped, not merged. */
  private epoch = 0;
  /** The first page while it's being fetched: navigation waits for it, so it never races it. */
  private initial: Promise<void> | null = null;
  /** The oldest event of the pages loaded so far: where the next page starts. Kept apart from
   *  the list, which live updates and navigation also add to, so paging never skips history. */
  private boundary: Pick<SpeechEvent, "startedAt" | "id"> | null = null;
  /** Whether history continues below `boundary` (mirrored in the timeline state). */
  private more = true;
  /** Updates to history below the pages loaded so far (speaker backfill, Transcribe anyway, a
   *  retry, a late-stored clip), newest version per id. Shown as soon as their page is in, so
   *  the list stays contiguous and a range or conversation copy never silently misses events. */
  private held = new Map<string, SpeechEvent>();
  /** Deleted while a page may still be on its way: a page answered before the delete must not
   *  bring them back. */
  private deleted = new Set<string>();
  private older: Promise<void> | null = null;
  /** The first page failed: nothing to anchor to, so live updates show rather than wait. */
  private initialFailed = false;
  /** When this start began: with no page to anchor to, only speech from about then on is shown
   *  live (older history still waits, so a retried first page places it without a hole). */
  private startedAt = Date.now();

  private resetTimeline() {
    this.boundary = null;
    this.more = true;
    this.held.clear();
    this.deleted.clear();
    this.initial = null;
    this.older = null;
    this.initialFailed = false;
    this.startedAt = Date.now();
    this.timeline.set({ events: [], loading: true, hasMore: true, error: null, arrivals: [] });
  }

  private forget(id: string) {
    this.held.delete(id);
    this.deleted.add(id);
    this.timeline.update((t) => ({ ...t, events: t.events.filter((e) => e.id !== id) }));
  }

  loadInitial(): Promise<void> {
    const run = this.fetchInitial();
    this.initial = run;
    void run.finally(() => {
      if (this.initial === run) this.initial = null;
    });
    return run;
  }
  private async fetchInitial() {
    const epoch = this.epoch;
    this.initialFailed = false;
    this.timeline.update((t) => ({ ...t, loading: true, error: null }));
    try {
      const page = await this.backend.listEvents(null, PAGE);
      if (epoch !== this.epoch) return;
      this.paged(page);
      this.timeline.update((t) => ({
        ...t,
        events: this.release(this.merge(t.events, page)),
        loading: false,
      }));
    } catch (e) {
      if (epoch !== this.epoch) return;
      this.initialFailed = !this.boundary;
      // No page to anchor to: show what arrived live rather than hold it with nothing to wait for.
      const live = this.boundary ? [] : this.takeHeld((ev) => this.isLive(ev));
      this.timeline.update((t) => ({ ...t, events: mergeEvents(t.events, live), loading: false, error: errorMessage(e) }));
    }
  }

  /** A page merged in, minus anything deleted while it was on its way. Versions already in the
   *  list came from later notifications than the page, so they win. */
  private merge(events: SpeechEvent[], page: SpeechEvent[]): SpeechEvent[] {
    const have = new Set(events.map((e) => e.id));
    return mergeEvents(events, page.filter((e) => !this.deleted.has(e.id) && !have.has(e.id)));
  }

  private paged(page: SpeechEvent[]) {
    if (page.length > 0) this.boundary = page[page.length - 1];
    this.more = page.length >= PAGE;
    this.timeline.update((t) => ({ ...t, hasMore: this.more }));
  }

  private takeHeld(due: (ev: SpeechEvent) => boolean): SpeechEvent[] {
    const out: SpeechEvent[] = [];
    for (const [id, ev] of this.held) {
      if (due(ev)) {
        out.push(ev);
        this.held.delete(id);
      }
    }
    return out;
  }

  /** `events` plus every held update now within the loaded pages (all of them at the end).
   *  Held versions came from notifications, so they replace a page's copy. */
  private release(events: SpeechEvent[]): SpeechEvent[] {
    const b = this.boundary;
    const due = this.takeHeld((ev) => !this.more || (b !== null && compareEvents(ev, b) >= 0));
    return due.length ? mergeEvents(events, due) : events;
  }

  /** Load the next page of history. Concurrent calls share the one in flight. */
  loadOlder(): Promise<void> {
    if (this.older) return this.older;
    const oldest = this.boundary;
    // No first page yet (it failed): scrolling up retries it.
    if (!oldest) return this.more && !this.initial ? this.loadInitial() : Promise.resolve();
    if (!this.more) return Promise.resolve();
    const epoch = this.epoch;
    let run!: Promise<void>;
    run = (async () => {
      this.timeline.update((t) => ({ ...t, loading: true }));
      try {
        const page = await this.backend.listEvents({ before: oldest.startedAt, beforeId: oldest.id }, PAGE);
        if (epoch !== this.epoch) return;
        this.paged(page);
        this.timeline.update((cur) => ({
          ...cur,
          events: this.release(this.merge(cur.events, page)),
          loading: false,
        }));
      } catch (e) {
        if (epoch !== this.epoch) return;
        // The boundary stays put, which is how a caller paging in a loop sees it failed.
        this.timeline.update((cur) => ({ ...cur, loading: false, error: errorMessage(e) }));
      } finally {
        if (this.older === run) this.older = null;
      }
    })();
    this.older = run;
    return run;
  }

  /** Ensure an event (e.g. a search hit) is loaded, paging back until it is. Null if it doesn't
   *  exist or its history couldn't be loaded. */
  async ensureLoaded(id: string): Promise<SpeechEvent | null> {
    if (this.initial) await this.initial;
    const find = () => this.timeline.get().events.find((e) => e.id === id) ?? null;
    if (find()) return find();
    if (!this.boundary && this.more) {
      // The first page failed (or never ran): try it once more, never show an island.
      await this.loadInitial();
      if (find()) return find();
      if (!this.boundary && this.more) return null;
    }
    const ev = await this.backend.getEvent(id);
    if (!ev || this.deleted.has(ev.id)) return null;
    // Load everything between the target and what we have, so the timeline stays contiguous
    // (every page, however far back: a range copy must never have a hole in it).
    while (this.more && this.boundary && compareEvents(this.boundary, ev) > 0) {
      const before = this.boundary;
      await this.loadOlder();
      if (this.boundary === before) break;
    }
    // Paging (or a notification) brought it in: that copy is at least as new as ours.
    if (find()) return find();
    if (this.more && this.boundary && compareEvents(ev, this.boundary) < 0) return null; // a page failed
    if (this.deleted.has(ev.id)) return null;
    const held = this.held.get(ev.id);
    this.held.delete(ev.id);
    this.timeline.update((t) => ({ ...t, events: mergeEvents(t.events, [held ?? ev]) }));
    return find();
  }

  /** Speech from this start on (with a margin for an utterance already under way). */
  private isLive(ev: SpeechEvent) {
    return ev.startedAt >= this.startedAt - 10 * 60_000;
  }

  private onUpsert(ev: SpeechEvent) {
    // Below the loaded pages, or before the first page is in: hold it until its page is.
    const waiting = this.boundary ? compareEvents(ev, this.boundary) < 0 : !(this.initialFailed && this.isLive(ev));
    if (this.more && waiting && !this.timeline.get().events.some((e) => e.id === ev.id)) {
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
