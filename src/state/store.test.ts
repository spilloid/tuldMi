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
  const backend = {
    async listEvents(cursor: EventCursor | null, limit: number) {
      return events
        .filter((e) => !cursor || compareEvents(e, { startedAt: cursor.before, id: cursor.beforeId }) < 0)
        .sort((a, b) => compareEvents(b, a))
        .slice(0, limit);
    },
    async getEvent(id: string) {
      return events.find((e) => e.id === id) ?? null;
    },
    on(name: string, handler: (payload: unknown) => void) {
      if (name === "tuldmi://event-upserted") upsert = handler as (e: SpeechEvent) => void;
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
  } as unknown as Backend;
  return { backend, events, announce: (e: SpeechEvent) => upsert?.(e) };
}

async function started(count: number) {
  const fake = fakeBackend(count);
  const store = new AppStore(fake.backend);
  await store.start();
  // start() kicks off the first page without awaiting it.
  await store.loadInitial();
  store.stop();
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
