import { describe, expect, it } from "vitest";
import { clamp01, levelToMeter, mapPeaks, peaksDurationMs, shapeHeight, xToTime } from "./peaks";

describe("mapPeaks", () => {
  it("handles empty input and zero bars", () => {
    expect(mapPeaks([], 10)).toHaveLength(10);
    expect(Array.from(mapPeaks([], 3))).toEqual([0, 0, 0]);
    expect(mapPeaks([255], 0)).toHaveLength(0);
  });

  it("maps 1:1 to normalised values", () => {
    expect(Array.from(mapPeaks([0, 51, 255], 3)).map((v) => +v.toFixed(4))).toEqual([0, 0.2, 1]);
  });

  it("downsamples with max so short transients survive", () => {
    const peaks = new Array(100).fill(10);
    peaks[57] = 255; // one loud click
    const bars = mapPeaks(peaks, 10);
    expect(bars).toHaveLength(10);
    expect(bars[5]).toBe(1);
    expect(bars[4]).toBeCloseTo(10 / 255);
  });

  it("covers every source sample exactly once when downsampling unevenly", () => {
    const peaks = Array.from({ length: 7 }, (_, i) => i * 30);
    const bars = mapPeaks(peaks, 3);
    // buckets: [0,1] [2,3] [4,5,6] -> max 30, 90, 180
    expect(Array.from(bars).map((v) => Math.round(v * 255))).toEqual([30, 90, 180]);
  });

  it("interpolates when upsampling", () => {
    const bars = mapPeaks([0, 255], 5);
    expect(Array.from(bars).map((v) => +v.toFixed(2))).toEqual([0, 0.25, 0.5, 0.75, 1]);
  });

  it("clamps out-of-range input", () => {
    expect(Array.from(mapPeaks([300, -4], 2))).toEqual([1, 0]);
  });
});

describe("helpers", () => {
  it("shapeHeight keeps a floor and is monotonic", () => {
    expect(shapeHeight(0)).toBeCloseTo(0.04);
    expect(shapeHeight(1)).toBe(1);
    expect(shapeHeight(0.25)).toBeGreaterThan(shapeHeight(0.1));
  });
  it("converts x to time and clamps", () => {
    expect(xToTime(50, 200, 8000)).toBe(2000);
    expect(xToTime(-10, 200, 8000)).toBe(0);
    expect(xToTime(999, 200, 8000)).toBe(8000);
    expect(xToTime(10, 0, 8000)).toBe(0);
  });
  it("peaks duration follows PEAKS_PER_SECOND", () => {
    expect(peaksDurationMs(50)).toBe(1000);
  });
  it("levelToMeter uses a dB scale", () => {
    expect(levelToMeter(0)).toBe(0);
    expect(levelToMeter(1)).toBe(1);
    expect(levelToMeter(0.001)).toBeCloseTo(0); // -60 dB
    expect(levelToMeter(0.0316)).toBeCloseTo(0.5, 1); // ~-30 dB
  });
  it("clamp01 handles NaN", () => {
    expect(clamp01(Number.NaN)).toBe(0);
  });
});
