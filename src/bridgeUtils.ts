import type { BridgeLog, BridgeSettings, ForwardEvent, LocationFix } from "./types";

export const MIN_POLL_INTERVAL_SECS = 2;

export type ActivityItem =
  | { kind: "forward"; timestampMs: number; event: ForwardEvent }
  | { kind: "log"; timestampMs: number; log: BridgeLog };

export type PreflightCheck = {
  label: string;
  ok: boolean;
};

export type CaltopoIdPreview = {
  value: string;
  source: "glympse" | "waiting" | "missing";
  label: string;
};

export function normalizePollInterval(value: unknown): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return MIN_POLL_INTERVAL_SECS;
  return Math.max(MIN_POLL_INTERVAL_SECS, Math.round(parsed));
}

export function buildMissingFields(settings: BridgeSettings): string[] {
  const missing: string[] = [];
  if (!settings.glympseSource.trim()) missing.push("a Glympse source");
  if (!settings.caltopoConnectKey.trim()) missing.push("a CalTopo connect key");
  return missing;
}

export function buildPreflightChecks(
  settings: BridgeSettings,
  latestLocation?: LocationFix | null
): PreflightCheck[] {
  const idPreview = buildCaltopoIdPreview(latestLocation);
  return [
    {
      label: settings.glympseSource.trim()
        ? `Glympse source: ${extractGlympseSourceCode(settings.glympseSource) || "entered"}`
        : "Glympse source is missing",
      ok: Boolean(settings.glympseSource.trim())
    },
    {
      label: settings.caltopoConnectKey.trim()
        ? `CalTopo connect key: ${settings.caltopoConnectKey.trim()}`
        : "CalTopo connect key is missing",
      ok: Boolean(settings.caltopoConnectKey.trim())
    },
    {
      label:
        latestLocation && idPreview.source === "missing"
          ? "Latest fix has no usable Glympse name"
          : `CalTopo IDs: ${idPreview.value}`,
      ok: !latestLocation || idPreview.source === "glympse"
    },
    {
      label: `Poll interval: ${normalizePollInterval(settings.pollIntervalSecs)} seconds`,
      ok: normalizePollInterval(settings.pollIntervalSecs) >= MIN_POLL_INTERVAL_SECS
    }
  ];
}

export function buildCaltopoIdPreview(latestLocation?: LocationFix | null): CaltopoIdPreview {
  if (latestLocation?.sourceLabel && isUsableGlympseIdentity(latestLocation.sourceLabel)) {
    return {
      value: normalizeCaltopoDeviceId(latestLocation.sourceLabel),
      source: "glympse",
      label: "from latest Glympse user"
    };
  }

  if (latestLocation) {
    return {
      value: "Not forwarded",
      source: "missing",
      label: "missing Glympse name"
    };
  }

  return {
    value: "Waiting for Glympse names",
    source: "waiting",
    label: "read from each active Glympse user"
  };
}

export function normalizeCaltopoDeviceId(value: string): string {
  return value
    .trim()
    .split("")
    .filter((character) => /[A-Za-z0-9]/.test(character))
    .join("");
}

export function isUsableGlympseIdentity(value: string): boolean {
  const trimmed = value.trim();
  if (!normalizeCaltopoDeviceId(trimmed)) return false;

  const lower = trimmed.toLowerCase();
  if (lower.startsWith("$")) return false;
  return !["embedded text", "glympse invite", "unnamed glympse user"].includes(lower);
}

export function extractGlympseSourceCode(source: string): string {
  const trimmed = source.trim();
  if (!trimmed) return "";

  try {
    const url = new URL(trimmed);
    for (const key of ["invite", "code", "g", "id", "ticket", "share", "link"]) {
      const value = url.searchParams.get(key);
      if (value?.trim()) return value.trim();
    }

    const ignored = new Set(["app", "ext", "g", "glympse", "invite", "map", "share", "ticket"]);
    const segments = url.pathname.split("/").filter(Boolean).reverse();
    for (const segment of segments) {
      const decoded = decodeURIComponent(segment).trim();
      if (decoded.length >= 2 && !ignored.has(decoded.toLowerCase())) return decoded;
    }
  } catch {
    return trimmed;
  }

  return trimmed;
}

export function buildActivityItems(forwards: ForwardEvent[], logs: BridgeLog[]): ActivityItem[] {
  return [
    ...forwards.map((event) => ({ kind: "forward" as const, timestampMs: event.timestampMs, event })),
    ...logs
      .filter((log) => !isForwardLog(log))
      .map((log) => ({ kind: "log" as const, timestampMs: log.timestampMs, log }))
  ]
    .sort((left, right) => right.timestampMs - left.timestampMs)
    .slice(0, 100);
}

export function formatTrackedUsers(locations: LocationFix[]): string {
  return locations
    .map((location) => location.sourceLabel || "Unnamed")
    .slice(0, 3)
    .join(", ")
    .concat(locations.length > 3 ? ` +${locations.length - 3}` : "");
}

export function formatForwardName(event: ForwardEvent): string {
  return event.sourceLabel || event.caltopoId;
}

export function formatList(values: string[]): string {
  if (values.length <= 1) return values[0] ?? "";
  if (values.length === 2) return `${values[0]} and ${values[1]}`;
  return `${values.slice(0, -1).join(", ")}, and ${values[values.length - 1]}`;
}

export function formatCoordinates(location: Pick<LocationFix, "lat" | "lng">): string {
  return `${location.lat.toFixed(5)}, ${location.lng.toFixed(5)}`;
}

export function formatLocationMeta(location: LocationFix): string {
  const parts = [
    location.timestampMs ? formatTime(location.timestampMs) : null,
    location.accuracy != null ? `${Math.round(location.accuracy)} m accuracy` : null,
    location.altitude != null ? `${Math.round(location.altitude)} m alt` : null,
    location.sourceLabel ? location.sourceLabel : null
  ].filter(Boolean);
  return parts.join(" | ") || "No metadata found";
}

export function formatTime(timestampMs: number): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit"
  }).format(new Date(timestampMs));
}

export function titleCase(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1)}`;
}

export function stringifyError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isForwardLog(log: BridgeLog): boolean {
  return (
    log.message.startsWith("CalTopo accepted") ||
    log.message.startsWith("CalTopo returned") ||
    log.message.startsWith("CalTopo request failed") ||
    log.message.startsWith("Location unchanged")
  );
}
