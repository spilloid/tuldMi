import type { Speaker, SpeechEvent, TranscriptSegment } from "./ipc/types";
import { compareEvents } from "./ipc/backend";

export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;

/** Events closer together than this belong to the same conversation. */
export const CONVERSATION_GAP_MS = 3 * MINUTE;

export function startOfDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function dayKey(t: number): string {
  const d = new Date(t);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** "14:05" — 24h, the mono gutter format. */
export function clock(t: number): string {
  const d = new Date(t);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** "14:05:09" */
export function clockSeconds(t: number): string {
  const d = new Date(t);
  return `${clock(t)}:${String(d.getSeconds()).padStart(2, "0")}`;
}

/** "0:07" / "1:02:33" for media offsets. */
export function mediaTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
}

/** Compact human duration: "45 s", "12 m", "2 h 14 m", "3 d 4 h". */
export function humanDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${Math.max(s, 1)} s`;
  const m = Math.round(ms / MINUTE);
  if (m < 60) return `${m} m`;
  const h = Math.floor(ms / HOUR);
  if (h < 24) {
    const rm = Math.round((ms - h * HOUR) / MINUTE);
    return rm ? `${h} h ${rm} m` : `${h} h`;
  }
  const d = Math.floor(ms / DAY);
  const rh = Math.round((ms - d * DAY) / HOUR);
  return rh ? `${d} d ${rh} h` : `${d} d`;
}

/** "Today", "Yesterday", "Tuesday", or "Tuesday, October 14" (with year if not this year). */
export function dayLabel(t: number, now: number = Date.now()): { title: string; detail: string } {
  const diff = Math.round((startOfDay(now) - startOfDay(t)) / DAY);
  const d = new Date(t);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  const full = d.toLocaleDateString(undefined, {
    month: "long",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
  const weekday = d.toLocaleDateString(undefined, { weekday: "long" });
  if (diff === 0) return { title: "Today", detail: `${weekday}, ${full}` };
  if (diff === 1) return { title: "Yesterday", detail: `${weekday}, ${full}` };
  return { title: weekday, detail: full };
}

export function relativeAgo(t: number, now: number = Date.now()): string {
  const diff = now - t;
  if (diff < 45_000) return "just now";
  if (diff < HOUR) return `${Math.round(diff / MINUTE)} min ago`;
  if (diff < DAY && startOfDay(t) === startOfDay(now)) return `${Math.round(diff / HOUR)} h ago`;
  const days = Math.round((startOfDay(now) - startOfDay(t)) / DAY);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  return new Date(t).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function shortDate(t: number): string {
  return new Date(t).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

// ---------------------------------------------------------------------------------------------
// Timeline grouping
// ---------------------------------------------------------------------------------------------

export interface Conversation {
  id: string; // first event id
  startedAt: number;
  endedAt: number;
  events: SpeechEvent[];
  sources: { mic: number; system: number };
}

export type TimelineRow =
  | { kind: "day"; key: string; at: number }
  | { kind: "gap"; key: string; from: number; to: number }
  | { kind: "conversation"; key: string; conversation: Conversation };

/**
 * Turn events (any order) into display rows, oldest first:
 * day headers, conversations (events with < gapMs between one's end and the next's start),
 * and gap rows between conversations on the same day.
 */
export function buildTimeline(events: SpeechEvent[], gapMs: number = CONVERSATION_GAP_MS): TimelineRow[] {
  const sorted = [...events].sort(compareEvents);
  const rows: TimelineRow[] = [];
  let current: Conversation | null = null;
  let currentDay: string | null = null;

  const flush = () => {
    if (current) rows.push({ kind: "conversation", key: `c:${current.id}`, conversation: current });
    current = null;
  };

  for (const ev of sorted) {
    const dk = dayKey(ev.startedAt);
    if (dk !== currentDay) {
      flush();
      rows.push({ kind: "day", key: `d:${dk}`, at: startOfDay(ev.startedAt) });
      currentDay = dk;
    } else if (current && ev.startedAt - (current as Conversation).endedAt >= gapMs) {
      const prevEnd: number = (current as Conversation).endedAt;
      flush();
      rows.push({ kind: "gap", key: `g:${ev.id}`, from: prevEnd, to: ev.startedAt });
    }
    if (!current) {
      current = {
        id: ev.id,
        startedAt: ev.startedAt,
        endedAt: ev.endedAt,
        events: [],
        sources: { mic: 0, system: 0 },
      };
    }
    const c = current as Conversation;
    c.events.push(ev);
    c.endedAt = Math.max(c.endedAt, ev.endedAt);
    c.sources[ev.source] += 1;
  }
  flush();
  return rows;
}

/** Merge a page or an upsert into a list, keyed by id; newer object wins. */
export function mergeEvents(existing: SpeechEvent[], incoming: SpeechEvent[]): SpeechEvent[] {
  const map = new Map(existing.map((e) => [e.id, e]));
  for (const e of incoming) map.set(e.id, e);
  return [...map.values()].sort(compareEvents);
}

export function eventText(ev: SpeechEvent): string {
  return ev.segments
    .map((s) => s.text.trim())
    .filter(Boolean)
    .join(" ");
}

export function sourceLabel(ev: Pick<SpeechEvent, "source" | "speaker">): string {
  return ev.speaker?.label ?? (ev.source === "mic" ? "Me" : "Remote");
}

// ---------------------------------------------------------------------------------------------
// Separated remote speakers (docs/design/speakers.md)
// ---------------------------------------------------------------------------------------------

/** Consecutive segments of one event said by one speaker. */
export interface SpeakerRun {
  /** A separated remote speaker, or null for the event's own speaker (Me / Remote). */
  speaker: Speaker | null;
  label: string;
  startMs: number;
  segments: TranscriptSegment[];
  text: string;
}

/** True when tuldMi separated at least one remote speaker in this event. */
export function isSeparated(ev: SpeechEvent): boolean {
  return ev.segments.some((s) => s.speaker != null);
}

/** The event's segments grouped by who said them, in order. Unseparated events give one run. */
export function speakerRuns(ev: SpeechEvent): SpeakerRun[] {
  const runs: SpeakerRun[] = [];
  for (const seg of ev.segments) {
    const text = seg.text.trim();
    if (!text) continue;
    const last = runs[runs.length - 1];
    if (last && (last.speaker?.id ?? null) === (seg.speaker?.id ?? null)) {
      last.segments.push(seg);
      last.text += " " + text;
    } else {
      runs.push({ speaker: seg.speaker, label: seg.speaker?.label ?? sourceLabel(ev), startMs: seg.startMs, segments: [seg], text });
    }
  }
  return runs;
}

/** Who speaks first and last in an event (for showing a label only when the speaker changes). */
export function speakerKeys(ev: SpeechEvent): { first: string; last: string } {
  const runs = speakerRuns(ev);
  const key = (r: SpeakerRun | undefined) => r?.speaker?.id ?? ev.source;
  return { first: key(runs[0]), last: key(runs[runs.length - 1]) };
}

/** Everyone heard in a set of events: Me, then remote speakers in order of first appearance. */
export function participants(events: SpeechEvent[]): string[] {
  const out: string[] = [];
  const add = (label: string) => !out.includes(label) && out.push(label);
  if (events.some((e) => e.source === "mic")) add("Me");
  for (const ev of events) {
    if (ev.source !== "system") continue;
    const runs = speakerRuns(ev);
    if (!runs.length) add(sourceLabel(ev));
    for (const run of runs) add(run.label);
  }
  return out;
}

/** "2026-11-09 14:02:11", the timestamp used when copying transcripts. */
export function stamp(t: number): string {
  const d = new Date(t);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** Events as copyable transcript text: one "[time] Speaker: text" line per speaker run. */
export function transcriptText(events: SpeechEvent[]): string {
  const lines: string[] = [];
  for (const ev of [...events].sort(compareEvents)) {
    for (const run of speakerRuns(ev)) lines.push(`[${stamp(ev.startedAt + run.startMs)}] ${run.label}: ${run.text}`);
  }
  return lines.join("\n");
}
