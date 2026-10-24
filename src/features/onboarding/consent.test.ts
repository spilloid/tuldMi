import { describe, expect, it } from "vitest";
import { consentPatch, isAcknowledged } from "./consent";

describe("consent", () => {
  it("stamps the acknowledgement time when ticked", () => {
    expect(consentPatch(true, 1234)).toEqual({ consentAcknowledgedAt: 1234 });
  });
  it("clears it when unticked", () => {
    expect(consentPatch(false, 1234)).toEqual({ consentAcknowledgedAt: null });
  });
  it("treats a stored timestamp as acknowledged and null as not", () => {
    expect(isAcknowledged({ consentAcknowledgedAt: 1 })).toBe(true);
    expect(isAcknowledged({ consentAcknowledgedAt: null })).toBe(false);
  });
});
