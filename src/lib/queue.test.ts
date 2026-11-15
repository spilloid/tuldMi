import { describe, expect, it } from "vitest";
import type { CaptureStatus } from "./ipc/types";
import { queueMessage } from "./queue";

describe("queue descriptions", () => {
  const status = { backlog: 10, transcriber: "working", queue: { fallingBehind: true, catchUpMs: null, oldestStartedAt: 1000 } } as CaptureStatus;
  it("does not promise a catch-up time when arrivals exceed capacity", () => {
    expect(queueMessage(status).title).toBe("Falling behind");
    expect(queueMessage(status).detail).toContain("No catch-up time");
  });
  it("does not show an old ETA while authentication or the engine is blocked", () => {
    expect(queueMessage({ ...status, transcriber: "error", queue: { ...status.queue, catchUpMs: 60_000 } }).title).toContain("needs attention");
  });
});
