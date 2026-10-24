// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMonitoringState } from "./useMonitoringState";

describe("useMonitoringState", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("is off when no test was requested, even after the grace period", () => {
    const { result } = renderHook(() => useMonitoringState(false, false));
    act(() => vi.advanceTimersByTime(5000));
    expect(result.current).toBe("off");
  });

  it("reports open as soon as the engine confirms", () => {
    const { result } = renderHook(() => useMonitoringState(true, true));
    expect(result.current).toBe("open");
  });

  it("warns only after the grace period when status.monitoring stays false", () => {
    const { result } = renderHook(() => useMonitoringState(true, false, 1500));
    expect(result.current).toBe("opening");
    act(() => vi.advanceTimersByTime(1499));
    expect(result.current).toBe("opening");
    act(() => vi.advanceTimersByTime(2));
    expect(result.current).toBe("failed");
  });

  it("recovers when the engine confirms late, and does not warn if it confirms in time", () => {
    const { result, rerender } = renderHook(({ m }) => useMonitoringState(true, m, 1500), { initialProps: { m: false } });
    act(() => vi.advanceTimersByTime(2000));
    expect(result.current).toBe("failed");
    rerender({ m: true });
    expect(result.current).toBe("open");
    rerender({ m: false });
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current).toBe("opening");
  });
});
