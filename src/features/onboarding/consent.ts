import type { Settings } from "../../lib/ipc/types";

export const isAcknowledged = (s: Pick<Settings, "consentAcknowledgedAt">): boolean => s.consentAcknowledgedAt != null;

/** Settings patch for ticking / unticking the consent box. */
export function consentPatch(checked: boolean, now: number = Date.now()): Pick<Settings, "consentAcknowledgedAt"> {
  return { consentAcknowledgedAt: checked ? now : null };
}
