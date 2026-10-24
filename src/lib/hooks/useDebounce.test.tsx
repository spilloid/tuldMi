// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDebouncedValue } from "./useDebounce";

describe("useDebouncedValue", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("returns the initial value immediately", () => {
    const { result } = renderHook(() => useDebouncedValue("a", 120));
    expect(result.current).toBe("a");
  });

  it("only updates after the value has been stable for the delay", () => {
    const { result, rerender } = renderHook(({ v }) => useDebouncedValue(v, 120), { initialProps: { v: "a" } });
    rerender({ v: "ab" });
    act(() => vi.advanceTimersByTime(80));
    rerender({ v: "abc" });
    act(() => vi.advanceTimersByTime(80));
    expect(result.current).toBe("a"); // 160 ms total, but never 120 ms stable
    act(() => vi.advanceTimersByTime(40));
    expect(result.current).toBe("abc");
  });

  it("passes through immediately with a zero delay", () => {
    const { result, rerender } = renderHook(({ v }) => useDebouncedValue(v, 0), { initialProps: { v: 1 } });
    rerender({ v: 2 });
    expect(result.current).toBe(2);
  });
});
