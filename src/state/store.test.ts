import { describe, expect, it } from "vitest";
import type { Backend, EventCursor } from "../lib/ipc/backend";
import { compareEvents } from "../lib/ipc/backend";
import type { SpeechEvent } from "../lib/ipc/types";
import { AppStore } from "./store";

function ev(i: number): SpeechEvent {
  return {
    id: `e${String(i).padStart(4, "0")}`,
    sessionId: "s",
    source: "system",
    speaker: null,
    startedAt: 1_000_000 + i * 10_000,
    endedAt: 1_000_000 + i * 10_000 + 4_000,
    status: "done",
    audio: null,
    audioExpired: false,
    segments: [],
    language: null,
    modelId: null,
    error: null,
  } as SpeechEvent;
}

/** A backend with `count` stored events that pages like the real one (newest first). */
function fakeBackend(count: number) {
  const events = Array.from({ length: count }, (_, i) => ev(i + 1));
  let upsert: ((e: SpeechEvent) => void) | null = null;
  let remove: ((id: string) => void) | null = null;
  /** When set, each page request waits for this before answering (from the data at answer time
   *  or, with `snapshot`, at request time). */
  const control: { gate: Promise<void> | null; snapshot: boolean; fail: boolean } = { gate: null, snapshot: false, fail: false };
  const backend = {
    async listEvents(cursor: EventCursor | null, limit: number) {
      const taken = control.snapshot ? [...events] : null;
      if (control.gate) await control.gate;
      if (control.fail) throw new Error("offline");
      return (taken ?? events)
        .filter((e) => !cursor || compareEvents(e, { startedAt: cursor.before, id: cursor.beforeId }) < 0)
        .sort((a, b) => compareEvents(b, a))
        .slice(0, limit);
    },
    async getEvent(id: string) {
      return events.find((e) => e.id === id) ?? null;
    },
    on(name: string, handler: (payload: unknown) => void) {
      if (name === "tuldmi://event-upserted") upsert = handler as (e: SpeechEvent) => void;
      if (name === "tuldmi://event-deleted") remove = handler as (id: string) => void;
      return () => {};
    },
    async getStatus() {
      return null;
    },
    async getSettings() {
      return null;
    },
    async listModels() {
      return [];
    },
    emitDelete: (id: string) => remove?.(id),
  } as unknown as Backend;
  return { backend, events, control, announce: (e: SpeechEvent) => upsert?.(e) };
}

async function started(count: number) {
  const fake = fakeBackend(count);
  const store = new AppStore(fake.backend);
  await store.start();
  // start() kicks off the first page without awaiting it.
  await store.loadInitial();
  return { store, ...fake };
}

describe("timeline paging with updates to older history", () => {
  it("an update to an unloaded event doesn't make paging skip history", async () => {
    const { store, events, announce } = await started(300);
    expect(store.timeline.get().events).toHaveLength(100);

    // Speaker backfill (or Transcribe anyway) finishes the very first event.
    announce({ ...events[0], speaker: { id: "p1", label: "Speaker 1", kind: "person" } } as SpeechEvent);
    expect(store.timeline.get().events).toHaveLength(100);

    await store.loadOlder();
    await store.loadOlder();
    const t = store.timeline.get();
    expect(t.events).toHaveLength(300);
    expect(t.events.map((e) => e.id)).toEqual(events.map((e) => e.id));
    await store.loadOlder();
    expect(store.timeline.get().hasMore).toBe(false);
  });

  it("updates to loaded events and new live events still apply", async () => {
    const { store, events, announce } = await started(300);
    const loaded = events[250];
    announce({ ...loaded, status: "empty" });
    expect(store.timeline.get().events.find((e) => e.id === loaded.id)?.status).toBe("empty");
    announce(ev(301));
    expect(store.timeline.get().events.at(-1)?.id).toBe(ev(301).id);
  });

  it("once all history is loaded, nothing is held back", async () => {
    const { store, events, announce } = await started(50);
    expect(store.timeline.get().hasMore).toBe(false);
    announce({ ...events[0], status: "empty" });
    expect(store.timeline.get().events[0].status).toBe("empty");
  });
});

function deferred() {
  let open!: () => void;
  const gate = new Promise<void>((r) => (open = r));
  return { gate, open };
}

function contiguous(store: AppStore, events: SpeechEvent[]) {
  const shown = store.timeline.get().events.map((e) => e.id);
  const first = events.findIndex((e) => e.id === shown[0]);
  expect(shown).toEqual(events.slice(first).map((e) => e.id));
}

describe("navigation to an old event", () => {
  it("pages all the way back, however far, with no hole", async () => {
    const { store, events } = await started(6000);
    const ev = await store.ensureLoaded(events[0].id);
    expect(ev?.id).toBe(events[0].id);
    expect(store.timeline.get().events).toHaveLength(6000);
    contiguous(store, events);
  });

  it("shares a page already being loaded instead of skipping ahead", async () => {
    const { store, events } = await started(1000);
    const scrolling = store.loadOlder();
    const ev = await store.ensureLoaded(events[0].id);
    await scrolling;
    expect(ev?.id).toBe(events[0].id);
    expect(store.timeline.get().events).toHaveLength(1000);
    contiguous(store, events);
  });

  it("a search jump during the first page keeps its target", async () => {
    const fake = fakeBackend(300);
    const store = new AppStore(fake.backend);
    const hold = deferred();
    fake.control.gate = hold.gate;
    const initial = store.loadInitial();
    const jump = store.ensureLoaded(fake.events[0].id);
    fake.control.gate = null;
    hold.open();
    await initial;
    const ev = await jump;
    expect(ev?.id).toBe(fake.events[0].id);
    expect(store.timeline.get().events.some((e) => e.id === fake.events[0].id)).toBe(true);
    contiguous(store, fake.events);
  });
});

describe("history stored late", () => {
  it("an older clip stored while the last page is in flight still shows", async () => {
    const fake = fakeBackend(150);
    const store = new AppStore(fake.backend);
    await store.start();
    await store.loadInitial();
    store.stop();
    // The last page answers from what was stored when it was asked for…
    const hold = deferred();
    fake.control.gate = hold.gate;
    fake.control.snapshot = true;
    const last = store.loadOlder();
    // …and meanwhile an older clip is stored and announced (a clock stepped back, say).
    const late = { ...ev(0), id: "e0000-late", startedAt: fake.events[0].startedAt - 5_000 };
    fake.events.unshift(late);
    fake.announce(late);
    hold.open();
    await last;
    const t = store.timeline.get();
    expect(t.hasMore).toBe(false);
    expect(t.events[0].id).toBe(late.id);
    expect(t.events).toHaveLength(151);
  });
});

describe("races with notifications", () => {
  it("a jump never replaces a newer version with its older snapshot", async () => {
    const { store, events, control, announce } = await started(300);
    const target = events[50];
    const hold = deferred();
    control.gate = hold.gate;
    const jump = store.ensureLoaded(target.id);
    await Promise.resolve();
    // While paging, the target is updated (speaker backfill finished it).
    const newer = { ...target, status: "empty" } as SpeechEvent;
    announce(newer);
    control.gate = null;
    hold.open();
    const ev = await jump;
    expect(ev?.status).toBe("empty");
    expect(store.timeline.get().events.find((e) => e.id === target.id)?.status).toBe("empty");
  });

  it("an event deleted while its page is on the way stays deleted", async () => {
    const fake = fakeBackend(300);
    const store = new AppStore(fake.backend);
    await store.start();
    await store.loadInitial();
    const hold = deferred();
    fake.control.gate = hold.gate;
    fake.control.snapshot = true;
    const older = store.loadOlder();
    const victim = fake.events[150];
    fake.events.splice(150, 1);
    (fake.backend as unknown as { emitDelete: (id: string) => void }).emitDelete(victim.id);
    hold.open();
    await older;
    store.stop();
    expect(store.timeline.get().events.some((e) => e.id === victim.id)).toBe(false);
  });

  it("if the first page fails, live speech still shows and a jump shows no island", async () => {
    const fake = fakeBackend(300);
    fake.control.fail = true;
    const store = new AppStore(fake.backend);
    await store.start();
    await store.loadInitial();
    const live = { ...ev(301), startedAt: Date.now(), endedAt: Date.now() + 4_000 };
    fake.events.push(live);
    fake.announce(live);
    expect(store.timeline.get().events.map((e) => e.id)).toEqual([live.id]);
    expect(await store.ensureLoaded(fake.events[0].id)).toBeNull();
    expect(store.timeline.get().events.map((e) => e.id)).toEqual([live.id]);
    // Back online: the retry fills in from the newest page down.
    fake.control.fail = false;
    expect((await store.ensureLoaded(fake.events[0].id))?.id).toBe(fake.events[0].id);
    store.stop();
    contiguous(store, fake.events);
  });

  it("start after stop re-reads the timeline instead of keeping a stale copy", async () => {
    const fake = fakeBackend(300);
    const store = new AppStore(fake.backend);
    await store.start();
    await store.loadInitial();
    store.stop();
    // Missed while stopped.
    for (let i = 301; i <= 500; i++) fake.events.push(ev(i));
    await store.start();
    await store.loadInitial();
    // Straight after the new first page: nothing kept from before the stop.
    contiguous(store, fake.events);
    expect(store.timeline.get().events).toHaveLength(100);
    expect(store.timeline.get().events.at(-1)?.id).toBe(ev(500).id);
    store.stop();
  });
});

describe("first page failed", () => {
  it("an update to old history waits, and the retry places it without a hole", async () => {
    const fake = fakeBackend(300);
    fake.control.fail = true;
    const store = new AppStore(fake.backend);
    await store.start();
    await store.loadInitial();
    // Old history (recorded long before this start) finishes in the background…
    const old = { ...fake.events[10], status: "empty" } as SpeechEvent;
    fake.events[10] = old;
    fake.announce(old);
    expect(store.timeline.get().events).toHaveLength(0);
    // …then scrolling up retries the first page once the backend is back.
    fake.control.fail = false;
    await store.loadOlder();
    expect(store.timeline.get().events.some((e) => e.id === old.id)).toBe(false);
    contiguous(store, fake.events);
    while (store.timeline.get().hasMore) await store.loadOlder();
    expect(store.timeline.get().events.find((e) => e.id === old.id)?.status).toBe("empty");
    contiguous(store, fake.events);
    store.stop();
  });
});
