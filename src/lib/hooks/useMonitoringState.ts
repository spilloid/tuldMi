import { useEffect, useState } from "react";

export const MONITORING_GRACE_MS = 1500;

export type MonitoringState = "off" | "opening" | "open" | "failed";

/**
 * Honest state of a device test. `requested` is whether the UI asked for monitoring; `monitoring`
 * is what the engine reports in `status.monitoring`. If the engine hasn't confirmed within the
 * grace period the devices couldn't be opened.
 */
export function useMonitoringState(requested: boolean, monitoring: boolean | undefined, graceMs = MONITORING_GRACE_MS): MonitoringState {
  const [late, setLate] = useState(false);
  useEffect(() => {
    setLate(false);
    if (!requested || monitoring) return;
    const t = setTimeout(() => setLate(true), graceMs);
    return () => clearTimeout(t);
  }, [requested, monitoring, graceMs]);
  if (!requested) return "off";
  if (monitoring) return "open";
  return late ? "failed" : "opening";
}
