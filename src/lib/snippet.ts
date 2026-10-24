import { HIGHLIGHT_END, HIGHLIGHT_START } from "./ipc/types";

export interface SnippetPart {
  text: string;
  hit: boolean;
}

/**
 * Split a search snippet on the \u0002 / \u0003 highlight delimiters into plain parts.
 * Robust to unbalanced or nested markers: a stray END outside a hit is dropped, an unclosed
 * START highlights to the end. Adjacent parts with the same flag are merged; empty parts dropped.
 * The result is rendered as text nodes — never as HTML.
 */
export function splitSnippet(snippet: string): SnippetPart[] {
  const parts: SnippetPart[] = [];
  let buf = "";
  let hit = false;
  const push = () => {
    if (!buf) return;
    const last = parts[parts.length - 1];
    if (last && last.hit === hit) last.text += buf;
    else parts.push({ text: buf, hit });
    buf = "";
  };
  for (const ch of snippet) {
    if (ch === HIGHLIGHT_START) {
      if (!hit) {
        push();
        hit = true;
      }
    } else if (ch === HIGHLIGHT_END) {
      if (hit) {
        push();
        hit = false;
      }
    } else {
      buf += ch;
    }
  }
  push();
  return parts;
}

/** Plain text of a snippet with markers stripped (for copy / aria). */
export function snippetText(snippet: string): string {
  return splitSnippet(snippet)
    .map((p) => p.text)
    .join("");
}
