import { HIGHLIGHT_END, HIGHLIGHT_START } from "./types";
import type { SearchHit, SearchQuery, SpeechEvent } from "./types";

/** Lowercased word tokens with their character ranges in the original string. */
export function tokenize(text: string): { word: string; start: number; end: number }[] {
  const out: { word: string; start: number; end: number }[] = [];
  const re = /[\p{L}\p{N}]+(?:['’][\p{L}]+)*/gu;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    out.push({ word: m[0].toLowerCase().replace(/’/g, "'"), start: m.index, end: m.index + m[0].length });
  }
  return out;
}

/** Query terms: words, lowercased, deduplicated. Every term must match (AND), by prefix. */
export function queryTerms(text: string): string[] {
  return [...new Set(tokenize(text).map((t) => t.word))];
}

const CONTEXT_WORDS = 7;

/**
 * Build an FTS5-style snippet: a window of words around the first match, matched words wrapped
 * in \u0002 … \u0003, ellipses where text was cut.
 */
export function buildSnippet(text: string, terms: string[]): string | null {
  const toks = tokenize(text);
  const isHit = toks.map((t) => terms.some((q) => t.word.startsWith(q)));
  const first = isHit.indexOf(true);
  if (first < 0) return null;
  const from = Math.max(0, first - CONTEXT_WORDS);
  const to = Math.min(toks.length - 1, first + CONTEXT_WORDS * 2);
  let out = from > 0 ? "…" : "";
  let cursor = toks[from].start;
  for (let i = from; i <= to; i++) {
    const t = toks[i];
    out += text.slice(cursor, t.start);
    out += isHit[i] ? HIGHLIGHT_START + text.slice(t.start, t.end) + HIGHLIGHT_END : text.slice(t.start, t.end);
    cursor = t.end;
  }
  if (to < toks.length - 1) out += "…";
  else out += text.slice(cursor);
  return out;
}

/** In-memory equivalent of the engine's FTS5 search, for the demo backend. */
export function searchEvents(events: SpeechEvent[], q: SearchQuery): SearchHit[] {
  const terms = queryTerms(q.text);
  if (terms.length === 0) return [];
  const hits: SearchHit[] = [];
  for (const ev of events) {
    if (q.sources.length && !q.sources.includes(ev.source)) continue;
    for (const seg of ev.segments) {
      const at = ev.startedAt + seg.startMs;
      if (q.from != null && at < q.from) continue;
      if (q.to != null && at > q.to) continue;
      const words = tokenize(seg.text).map((t) => t.word);
      if (!terms.every((term) => words.some((w) => w.startsWith(term)))) continue;
      const snippet = buildSnippet(seg.text, terms);
      if (!snippet) continue;
      const matches = words.filter((w) => terms.some((term) => w.startsWith(term))).length;
      const exact = words.filter((w) => terms.includes(w)).length;
      // FTS5 convention: lower rank = better.
      const rank = -(matches + exact * 0.5) / Math.sqrt(words.length + 4);
      hits.push({ eventId: ev.id, segmentId: seg.id, source: ev.source, at, snippet, rank });
    }
  }
  hits.sort((a, b) => a.rank - b.rank || b.at - a.at);
  return hits.slice(0, q.limit ?? 50);
}
