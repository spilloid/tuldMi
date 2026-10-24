import { describe, expect, it } from "vitest";
import { splitSnippet, snippetText } from "./snippet";

const S = "\u0002";
const E = "\u0003";

describe("splitSnippet", () => {
  it("splits plain and highlighted runs", () => {
    expect(splitSnippet(`the ${S}budget${E} line for ${S}contractors${E}.`)).toEqual([
      { text: "the ", hit: false },
      { text: "budget", hit: true },
      { text: " line for ", hit: false },
      { text: "contractors", hit: true },
      { text: ".", hit: false },
    ]);
  });

  it("returns a single plain part without markers", () => {
    expect(splitSnippet("no markers here")).toEqual([{ text: "no markers here", hit: false }]);
  });

  it("handles a highlight at the very start and end", () => {
    expect(splitSnippet(`${S}all${E}`)).toEqual([{ text: "all", hit: true }]);
  });

  it("merges adjacent highlights and drops empty ones", () => {
    expect(splitSnippet(`${S}a${E}${S}b${E}${S}${E} c`)).toEqual([
      { text: "ab", hit: true },
      { text: " c", hit: false },
    ]);
  });

  it("is robust to unbalanced markers", () => {
    expect(splitSnippet(`x ${E}y`)).toEqual([{ text: "x y", hit: false }]);
    expect(splitSnippet(`x ${S}y ${S}z`)).toEqual([
      { text: "x ", hit: false },
      { text: "y z", hit: true },
    ]);
  });

  it("never interprets HTML", () => {
    const parts = splitSnippet(`<img src=x onerror=alert(1)> ${S}<b>${E}`);
    expect(parts[0]).toEqual({ text: "<img src=x onerror=alert(1)> ", hit: false });
    expect(parts[1]).toEqual({ text: "<b>", hit: true });
  });

  it("strips markers for plain text", () => {
    expect(snippetText(`…said ${S}hello${E} there…`)).toBe("…said hello there…");
  });
});
