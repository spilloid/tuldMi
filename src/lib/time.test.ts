import { describe, expect, it } from "vitest";
import type { SpeechEvent } from "./ipc/types";
import { CONVERSATION_GAP_MS, buildTimeline, humanDuration, mediaTime, mergeEvents } from "./time";

let n = 0;
function ev(startedAt: number, durMs = 4000, source: "mic" | "system" = "mic"): SpeechEvent {
  n++;
  return {
    id: `e${String(n).padStart(3, "0")}`,
    sessionId: "s",
    source,
    speaker: null,
    startedAt,
    endedAt: startedAt + durMs,
    status: "done",
    audio: null,
    audioExpired: false,
    segments: [],
    language: null,
    modelId: null,
    error: null,
  };
}

const day = (d: number, h: number, m = 0, s = 0) => new Date(2026, 9, d, h, m, s).getTime();

describe("buildTimeline", () => {
  it("returns nothing for no events", () => {
    expect(buildTimeline([])).toEqual([]);
  });

  it("clusters events closer than the gap into one conversation", () => {
    const a = ev(day(20, 10, 0));
    const b = ev(day(20, 10, 1)); // 56 s after a ends
    const c = ev(day(20, 10, 3, 30)); // 2m26s after b ends — still < 3 min
    const rows = buildTimeline([c, a, b]);
    expect(rows.map((r) => r.kind)).toEqual(["day", "conversation"]);
    const conv = rows[1];
    if (conv.kind !== "conversation") throw new Error();
    expect(conv.conversation.events.map((e) => e.id)).toEqual([a.id, b.id, c.id]);
    expect(conv.conversation.startedAt).toBe(a.startedAt);
    expect(conv.conversation.endedAt).toBe(c.endedAt);
  });

  it("splits at exactly the gap threshold and inserts a gap row with the quiet span", () => {
    const a = ev(day(20, 9, 0), 5000);
    const b = ev(a.endedAt + CONVERSATION_GAP_MS);
    const rows = buildTimeline([a, b]);
    expect(rows.map((r) => r.kind)).toEqual(["day", "conversation", "gap", "conversation"]);
    const gap = rows[2];
    if (gap.kind !== "gap") throw new Error();
    expect(gap.from).toBe(a.endedAt);
    expect(gap.to).toBe(b.startedAt);
  });

  it("measures the gap from the latest end in the cluster, not the last start", () => {
    const long = ev(day(20, 9, 0), 10 * 60_000); // 10 minutes
    const short = ev(day(20, 9, 1), 3000); // overlaps (other source)
    const next = ev(long.endedAt + 60_000); // 1 min after long ends
    const rows = buildTimeline([long, short, next]);
    expect(rows.filter((r) => r.kind === "conversation")).toHaveLength(1);
  });

  it("starts a new day header (and no gap row) across midnight", () => {
    const a = ev(day(20, 23, 59));
    const b = ev(day(21, 0, 1));
    const rows = buildTimeline([a, b]);
    expect(rows.map((r) => r.kind)).toEqual(["day", "conversation", "day", "conversation"]);
  });

  it("counts sources per conversation", () => {
    const rows = buildTimeline([ev(day(20, 8), 2000, "mic"), ev(day(20, 8, 0, 10), 2000, "system"), ev(day(20, 8, 0, 20), 2000, "system")]);
    const conv = rows.find((r) => r.kind === "conversation");
    if (!conv || conv.kind !== "conversation") throw new Error();
    expect(conv.conversation.sources).toEqual({ mic: 1, system: 2 });
  });
});

describe("mergeEvents", () => {
  it("upserts by id and keeps chronological order", () => {
    const a = ev(day(20, 8));
    const b = ev(day(20, 9));
    const b2 = { ...b, status: "failed" as const };
    const c = ev(day(20, 7));
    const merged = mergeEvents([a, b], [b2, c]);
    expect(merged.map((e) => e.id)).toEqual([c.id, a.id, b.id]);
    expect(merged[2].status).toBe("failed");
  });
});

describe("durations", () => {
  it("formats human durations compactly", () => {
    expect(humanDuration(800)).toBe("1 s");
    expect(humanDuration(45_000)).toBe("45 s");
    expect(humanDuration(12 * 60_000)).toBe("12 m");
    expect(humanDuration(2 * 3_600_000 + 14 * 60_000)).toBe("2 h 14 m");
    expect(humanDuration(3 * 3_600_000)).toBe("3 h");
    expect(humanDuration(26 * 3_600_000)).toBe("1 d 2 h");
  });
  it("formats media time", () => {
    expect(mediaTime(7_400)).toBe("0:07");
    expect(mediaTime(62_000)).toBe("1:02");
    expect(mediaTime(3_753_000)).toBe("1:02:33");
    expect(mediaTime(-5)).toBe("0:00");
  });
});
