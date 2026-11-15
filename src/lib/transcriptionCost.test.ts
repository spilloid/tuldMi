import { describe, expect, it } from "vitest";
import { transcriptionDayCost } from "./transcriptionCost";
describe("8-hour transcription costs", () => {
  it("counts both retained tracks and prorates speech activity", () => {
    expect(transcriptionDayCost("gpt-transcribe", 8, 1, 100)?.dollars).toBeCloseTo(2.16);
    expect(transcriptionDayCost("gpt-transcribe", 8, 2, 100)?.dollars).toBeCloseTo(4.32);
    const quarter = transcriptionDayCost("gpt-transcribe", 8, 2, 25);
    expect(quarter?.audioMinutes).toBe(240);
    expect(quarter?.dollars).toBeCloseTo(1.08);
    expect(transcriptionDayCost("gpt-4o-mini-transcribe", 8, 2, 100)?.dollars).toBeCloseTo(2.88);
  });
  it("does not invent Claude audio pricing or invalid estimates", () => {
    expect(transcriptionDayCost("claude-sonnet-5-5", 8, 2, 100)).toBeNull();
    expect(transcriptionDayCost("gpt-transcribe", 8, 2, 101)).toBeNull();
    expect(transcriptionDayCost("gpt-transcribe", 8, 2, NaN)).toBeNull();
  });
});
