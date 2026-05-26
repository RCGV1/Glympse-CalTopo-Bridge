import { describe, expect, it } from "vitest";
import {
  buildCaltopoIdPreview,
  buildMissingFields,
  extractGlympseSourceCode,
  isUsableGlympseIdentity,
  normalizeCaltopoDeviceId,
  normalizePollInterval
} from "./bridgeUtils";
import type { BridgeSettings, LocationFix } from "./types";

function settings(overrides: Partial<BridgeSettings> = {}): BridgeSettings {
  return {
    glympseSource: "https://glympse.com/!ABC123",
    caltopoConnectKey: "Sequoia",
    caltopoDeviceId: "",
    pollIntervalSecs: 5,
    forwardUnchanged: false,
    includeAltitude: true,
    ...overrides
  };
}

function location(overrides: Partial<LocationFix> = {}): LocationFix {
  return {
    lat: 36.47375,
    lng: -118.85302,
    sourceLabel: "Ben Ko6cnt",
    ...overrides
  };
}

describe("bridge utility helpers", () => {
  it("extracts Glympse codes from URLs and raw input", () => {
    expect(extractGlympseSourceCode("https://glympse.com/!ABC123")).toBe("!ABC123");
    expect(extractGlympseSourceCode("https://example.test/share?invite=XYZ789")).toBe("XYZ789");
    expect(extractGlympseSourceCode("RAWCODE")).toBe("RAWCODE");
  });

  it("normalizes CalTopo IDs the same way the backend does", () => {
    expect(normalizeCaltopoDeviceId("Ben Ko6cnt")).toBe("BenKo6cnt");
    expect(normalizeCaltopoDeviceId("BBEV-uqvl")).toBe("BBEVuqvl");
    expect(normalizeCaltopoDeviceId(" $ / ")).toBe("");
  });

  it("prefers a real Glympse name over the manual fallback", () => {
    expect(
      buildCaltopoIdPreview(settings({ caltopoDeviceId: "Manual Fallback" }), location())
    ).toEqual({
      value: "BenKo6cnt",
      source: "glympse",
      label: "from latest Glympse user"
    });
  });

  it("uses the manual fallback for generic parsed labels", () => {
    expect(
      buildCaltopoIdPreview(
        settings({ caltopoDeviceId: "Manual Fallback" }),
        location({ sourceLabel: "$.response.location" })
      )
    ).toEqual({
      value: "ManualFallback",
      source: "fallback",
      label: "manual fallback"
    });
  });

  it("has a deterministic default CalTopo ID when no better ID is known", () => {
    expect(buildCaltopoIdPreview(settings(), location({ sourceLabel: "embedded text" }))).toEqual({
      value: "Glympse",
      source: "default",
      label: "default fallback"
    });
  });

  it("flags generic labels as poor track identities", () => {
    expect(isUsableGlympseIdentity("Ben Ko6cnt")).toBe(true);
    expect(isUsableGlympseIdentity("embedded text")).toBe(false);
    expect(isUsableGlympseIdentity("$.result.location")).toBe(false);
  });

  it("normalizes short, empty, and fractional poll intervals", () => {
    expect(normalizePollInterval(1)).toBe(2);
    expect(normalizePollInterval("3.4")).toBe(3);
    expect(normalizePollInterval("bad")).toBe(2);
  });

  it("reports only required start fields as missing", () => {
    expect(buildMissingFields(settings({ glympseSource: "", caltopoConnectKey: "" }))).toEqual([
      "a Glympse source",
      "a CalTopo connect key"
    ]);
  });
});
