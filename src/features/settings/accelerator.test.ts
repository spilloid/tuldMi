import { describe, expect, it } from "vitest";
import { acceleratorFromEvent } from "./sections";
import { acceleratorParts } from "../../components/ui/Kbd";

const k = (code: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) => ({
  code,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("acceleratorFromEvent", () => {
  it("builds Tauri accelerators", () => {
    expect(acceleratorFromEvent(k("Backquote", { ctrlKey: true, shiftKey: true }), false)).toBe("CommandOrControl+Shift+Backquote");
    expect(acceleratorFromEvent(k("KeyP", { altKey: true }), false)).toBe("Alt+P");
    expect(acceleratorFromEvent(k("Digit9", { metaKey: true }), true)).toBe("CommandOrControl+9");
    expect(acceleratorFromEvent(k("F9"), false)).toBe("F9");
  });
  it("rejects shortcuts that would eat normal typing", () => {
    expect(acceleratorFromEvent(k("KeyP"), false)).toBeNull();
    expect(acceleratorFromEvent(k("KeyP", { shiftKey: true }), false)).toBeNull();
    expect(acceleratorFromEvent(k("ShiftLeft", { shiftKey: true }), false)).toBeNull();
  });
  it("renders accelerators as readable keycaps", () => {
    expect(acceleratorParts("CommandOrControl+Shift+Backquote")).toEqual(["Ctrl", "Shift", "`"]);
  });
});
