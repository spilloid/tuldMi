import { describe, expect, it, vi } from "vitest";
import { AppStore } from "./store";
import type { Backend, BackendEvents } from "../lib/ipc/backend";
import { buildDemoEvents } from "../lib/ipc/demoData";

describe("late transcription updates", () => {
  it("preserves a completion delivered while an older page is in flight", async () => {
    const template = buildDemoEvents(Date.now()).events[0];
    const events = Array.from({ length: 250 }, (_, i) => ({ ...template, status: "queued" as const, id: `event-${String(i).padStart(3, "0")}`, startedAt: i * 1000, endedAt: i * 1000 + 500 }));
    let upsert: (ev: BackendEvents["tuldmi://event-upserted"]) => void = () => {};
    let resolveOlder!: (page: typeof events) => void;
    const older = new Promise<typeof events>((resolve) => { resolveOlder = resolve; });
    const backend = {
      listEvents: async (cursor: unknown) => cursor ? older : events.slice(-100).reverse(),
      getStatus: async () => ({}), getSettings: async () => ({}), listModels: async () => [],
      on: (name: string, fn: typeof upsert) => { if (name === "tuldmi://event-upserted") upsert = fn; return () => {}; },
    } as unknown as Backend;
    const store = new AppStore(backend);
    await store.start();
    await vi.waitFor(() => expect(store.timeline.get().loading).toBe(false));
    const loading = store.loadOlder();
    upsert({ ...events[125], status: "done" });
    expect(store.timeline.get().events[0].id).toBe(events[150].id);
    resolveOlder(events.slice(50, 150).reverse());
    await loading;
    expect(store.timeline.get().events.find((e) => e.id === events[125].id)?.status).toBe("done");
    expect(store.timeline.get().events[0].id).toBe(events[50].id);
    store.stop();
  });
  it("keeps the loaded range contiguous when an old queued recording finishes", async () => {
    const template = buildDemoEvents(Date.now()).events[0];
    const events = Array.from({ length: 250 }, (_, i) => ({ ...template, id: `event-${String(i).padStart(3, "0")}`, startedAt: i * 1000, endedAt: i * 1000 + 500 }));
    let upsert: (ev: BackendEvents["tuldmi://event-upserted"]) => void = () => {};
    const listEvents = vi.fn(async (cursor) => events.filter((e) => !cursor || e.startedAt < cursor.before).slice(-100).reverse());
    const backend = {
      listEvents,
      getStatus: async () => ({}), getSettings: async () => ({}), listModels: async () => [],
      on: (name: string, fn: typeof upsert) => { if (name === "tuldmi://event-upserted") upsert = fn; return () => {}; },
    } as unknown as Backend;
    const store = new AppStore(backend);
    await store.start();
    await vi.waitFor(() => expect(store.timeline.get().loading).toBe(false));
    upsert({ ...events[10], status: "done" });
    expect(store.timeline.get().events[0].id).toBe(events[150].id);
    await store.loadOlder();
    expect(listEvents.mock.calls[1][0].before).toBe(150_000);
    expect(store.timeline.get().events).toHaveLength(200);
    upsert({ ...events[175], status: "done" });
    expect(store.timeline.get().events.find((e) => e.id === events[175].id)?.status).toBe("done");
    store.stop();
  });
});
