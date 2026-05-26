import { describe, expect, it } from "vitest";
import {
  buildCaltopoIdPreview,
  buildMissingFields,
  buildPreflightChecks,
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

  it("previews the CalTopo ID from a real Glympse name", () => {
    expect(buildCaltopoIdPreview(location())).toEqual({
      value: "BenKo6cnt",
      source: "glympse",
      label: "from latest Glympse user"
    });
  });

  it("does not invent a CalTopo ID for generic parsed labels", () => {
    expect(buildCaltopoIdPreview(location({ sourceLabel: "$.response.location" }))).toEqual({
      value: "Not forwarded",
      source: "missing",
      label: "missing Glympse name"
    });
  });

  it("fails preflight once the latest fix has no usable Glympse name", () => {
    expect(
      buildPreflightChecks(settings(), location({ sourceLabel: "embedded text" })).find((check) =>
        check.label.includes("Glympse name")
      )
    ).toEqual({
      label: "Latest fix has no usable Glympse name",
      ok: false
    });
  });

  it("waits for Glympse names before previewing a CalTopo ID", () => {
    expect(buildCaltopoIdPreview()).toEqual({
      value: "Waiting for Glympse names",
      source: "waiting",
      label: "read from each active Glympse user"
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
