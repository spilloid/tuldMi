import { describe, expect, it } from "vitest";
import type { Speaker, SpeechEvent, TranscriptSegment } from "./ipc/types";
import { CONVERSATION_GAP_MS, buildTimeline, humanDuration, mediaTime, mergeEvents, participants, speakerKeys, speakerRuns, transcriptText } from "./time";

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

describe("separated speakers", () => {
  const person = (id: string, label: string): Speaker => ({ id, label, kind: "person" });
  const seg = (startMs: number, text: string, speaker: Speaker | null = null): TranscriptSegment => ({
    id: `g${startMs}`,
    eventId: "x",
    startMs,
    endMs: startMs + 900,
    text,
    confidence: null,
    speaker,
  });
  const dana = person("p1", "Dana");
  const two = person("p2", "Speaker 2");
  const at = day(9, 14, 2, 11);

  it("groups consecutive segments by speaker, unseparated speech keeps the source label", () => {
    const e = { ...ev(at, 6000, "system"), segments: [seg(0, "Ship it.", dana), seg(1000, "Friday.", dana), seg(2000, " ", two), seg(3000, "Hm."), seg(4000, "Agreed.", two)] };
    expect(speakerRuns(e).map((r) => [r.label, r.text, r.startMs])).toEqual([
      ["Dana", "Ship it. Friday.", 0],
      ["Remote", "Hm.", 3000],
      ["Speaker 2", "Agreed.", 4000],
    ]);
    expect(speakerKeys(e)).toEqual({ first: "p1", last: "p2" });
    const plain = { ...ev(at, 1000, "mic"), segments: [seg(0, "Hello.")] };
    expect(speakerKeys(plain)).toEqual({ first: "mic", last: "mic" });
  });

  it("lists who took part: Me first, then remote speakers in order", () => {
    const a = { ...ev(at, 1000, "system"), segments: [seg(0, "One.", two)] };
    const b = { ...ev(at + 2000, 1000, "mic"), segments: [seg(0, "Two.")] };
    const c = { ...ev(at + 4000, 1000, "system"), segments: [seg(0, "Three.", dana), seg(500, "Four.")] };
    expect(participants([a, b, c])).toEqual(["Me", "Speaker 2", "Dana", "Remote"]);
  });

  it("copies events as one timestamped line per speaker run, in time order", () => {
    const a = { ...ev(at, 6000, "system"), segments: [seg(0, "Ship it.", dana), seg(2500, "Agreed.", two)] };
    const b = { ...ev(at + 7000, 1000, "mic"), segments: [seg(0, "Done.")] };
    expect(transcriptText([b, a])).toBe(
      ["[2026-10-09 14:02:11] Dana: Ship it.", "[2026-10-09 14:02:13] Speaker 2: Agreed.", "[2026-10-09 14:02:18] Me: Done."].join("\n"),
    );
  });
});
