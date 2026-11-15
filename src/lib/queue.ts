import type { CaptureStatus } from "./ipc/types";
import { clock, humanDuration } from "./time";

export function queueMessage(status: CaptureStatus): { title: string; detail: string } {
  const q = status.queue;
  if (!status.backlog) return { title: "Up to date", detail: "No recordings waiting for transcription." };
  const oldest = q?.oldestStartedAt != null ? ` Oldest recording: ${clock(q.oldestStartedAt)}.` : "";
  if (status.transcriber === "noModel") return { title: "Waiting for a model", detail: `Install or select a model to start processing.${oldest}` };
  if (status.transcriber === "error") return { title: "Transcription needs attention", detail: `Recordings remain saved. Check server sign-in or model settings.${oldest}` };
  if (q?.fallingBehind) return { title: "Falling behind", detail: `Speech is arriving faster than it can be transcribed. No catch-up time at this rate. Use server processing or a smaller local model.${oldest}` };
  if (q?.catchUpMs != null) return { title: `Catch up in ~${humanDuration(q.catchUpMs)}`, detail: `Estimate includes ongoing incoming speech. It changes as the workload changes.${oldest}` };
  return { title: "Measuring processing speed", detail: `A catch-up estimate needs several completed clips and 30 seconds of capture.${oldest}` };
}
