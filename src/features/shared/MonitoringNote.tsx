import { AlertTriangle, Mic } from "lucide-react";
import { useMonitoringState } from "../../lib/hooks/useMonitoringState";
import { useStatus } from "../../state/hooks";

/** Small, honest state line for a device test. Renders nothing until a test is requested. */
export function MonitoringNote({ requested }: { requested: boolean }) {
  const status = useStatus();
  const state = useMonitoringState(requested, status?.monitoring);
  if (state === "off") return null;
  if (state === "failed") {
    return (
      <p role="status" className="mt-3 flex items-center gap-2 text-sm text-warning">
        <AlertTriangle size={13} aria-hidden /> Couldn't open the devices for testing. Check that a microphone or output is connected and not in exclusive use.
      </p>
    );
  }
  return (
    <p role="status" className="mt-3 flex items-center gap-2 text-sm text-muted">
      <Mic size={13} aria-hidden className={state === "open" ? "text-success" : "text-faint"} />
      {state === "open" ? "Devices open for testing — nothing is recorded." : "Opening devices…"}
    </p>
  );
}
