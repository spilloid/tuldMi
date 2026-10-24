import { describe, expect, it } from "vitest";
import { buildDemoEvents } from "./demoData";
import { buildSnippet, queryTerms, searchEvents } from "./demoSearch";
import { DemoBackend } from "./demo";
import { HIGHLIGHT_END, HIGHLIGHT_START } from "./types";
import type { SearchQuery } from "./types";
import { splitSnippet } from "../snippet";

const NOW = new Date(2026, 9, 23, 15, 0, 0).getTime();
const q = (text: string, extra: Partial<SearchQuery> = {}): SearchQuery => ({ text, sources: [], from: null, to: null, limit: null, ...extra });

describe("demo search", () => {
  const { events } = buildDemoEvents(NOW);

  it("parses query terms case-insensitively and de-duplicates", () => {
    expect(queryTerms("Budget  budget, Line")).toEqual(["budget", "line"]);
    expect(queryTerms("  ")).toEqual([]);
  });

  it("finds words by prefix and highlights with the engine delimiters", () => {
    const hits = searchEvents(events, q("contract"));
    expect(hits.length).toBeGreaterThan(0);
    for (const h of hits) {
      expect(h.snippet).toContain(HIGHLIGHT_START);
      expect(h.snippet).toContain(HIGHLIGHT_END);
      const highlighted = splitSnippet(h.snippet).filter((p) => p.hit).map((p) => p.text.toLowerCase());
      expect(highlighted.every((w) => w.startsWith("contract"))).toBe(true);
    }
  });

  it("requires every term (AND)", () => {
    const both = searchEvents(events, q("henderson proposal"));
    const one = searchEvents(events, q("henderson"));
    expect(both.length).toBeGreaterThan(0);
    expect(both.length).toBeLessThanOrEqual(one.length);
    expect(searchEvents(events, q("henderson xylophone"))).toEqual([]);
  });

  it("filters by source", () => {
    const mic = searchEvents(events, q("the", { sources: ["mic"] }));
    const sys = searchEvents(events, q("the", { sources: ["system"] }));
    expect(mic.length).toBeGreaterThan(0);
    expect(sys.length).toBeGreaterThan(0);
    expect(mic.every((h) => h.source === "mic")).toBe(true);
    expect(sys.every((h) => h.source === "system")).toBe(true);
  });

  it("filters by date range using the segment time", () => {
    const from = NOW - 2 * 86_400_000;
    const hits = searchEvents(events, q("the", { from }));
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.every((h) => h.at >= from)).toBe(true);
    const old = searchEvents(events, q("webinar", { from }));
    expect(old).toEqual([]);
  });

  it("orders by rank (lower is better) and respects limit", () => {
    const hits = searchEvents(events, q("the", { limit: 5 }));
    expect(hits).toHaveLength(5);
    for (let i = 1; i < hits.length; i++) expect(hits[i].rank).toBeGreaterThanOrEqual(hits[i - 1].rank);
  });

  it("does not match queued, transcribing or failed events (no text yet)", () => {
    const pendingIds = new Set(events.filter((e) => e.status !== "done").map((e) => e.id));
    const hits = searchEvents(events, q("budget"));
    expect(hits.some((h) => pendingIds.has(h.eventId))).toBe(false);
  });

  it("builds windowed snippets with ellipses", () => {
    const text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight";
    const s = buildSnippet(text, ["twelve"])!;
    expect(s.startsWith("…")).toBe(true);
    expect(s.endsWith("…")).toBe(true);
    expect(s).toContain(`${HIGHLIGHT_START}twelve${HIGHLIGHT_END}`);
    expect(buildSnippet(text, ["zzz"])).toBeNull();
  });

  it("DemoBackend.search returns the same results and never fabricates audio", async () => {
    const b = new DemoBackend({ now: NOW, still: true });
    const hits = await b.search(q("roadmap"));
    expect(hits.length).toBeGreaterThan(0);
    const all = await b.listEvents(null, 500);
    expect(all.every((e) => e.audio === null)).toBe(true);
    expect(all.some((e) => e.audioExpired)).toBe(true);
    expect(all.some((e) => e.status === "failed")).toBe(true);
    expect(all.some((e) => e.status === "transcribing")).toBe(true);
    expect(all.some((e) => e.status === "empty")).toBe(true);
    b.dispose();
  });

  it("listEvents pages newest-first with a before cursor", async () => {
    const b = new DemoBackend({ now: NOW, still: true });
    const first = await b.listEvents(null, 10);
    expect(first).toHaveLength(10);
    for (let i = 1; i < first.length; i++) expect(first[i].startedAt).toBeLessThanOrEqual(first[i - 1].startedAt);
    const next = await b.listEvents(first[first.length - 1].startedAt, 10);
    expect(next.every((e) => e.startedAt < first[first.length - 1].startedAt)).toBe(true);
    b.dispose();
  });
});
