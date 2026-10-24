import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { PACKAGE_VERSION, resolveAppVersion } from "./version";

describe("resolveAppVersion", () => {
  it("falls back to package.json outside Tauri", async () => {
    expect(await resolveAppVersion(false)).toBe(pkg.version);
    expect(PACKAGE_VERSION).toBe(pkg.version);
  });
  it("uses getVersion in Tauri and falls back if it fails", async () => {
    expect(await resolveAppVersion(true, async () => "9.9.9")).toBe("9.9.9");
    expect(await resolveAppVersion(true, async () => { throw new Error("x"); })).toBe(pkg.version);
  });
});
