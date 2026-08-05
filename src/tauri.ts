import type {
  BridgeLog,
  BridgeSettings,
  BridgeStatus,
  ForwardEvent,
  GlympseDiagnostics,
  LocationFix,
  PollOutcome
} from "./types";

type Unlisten = () => void;

const isTauri = "__TAURI_INTERNALS__" in window;

async function invokeTauri<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    return demoInvoke<T>(command);
  }

  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

async function listenTauri<T>(
  eventName: string,
  handler: (payload: T) => void
): Promise<Unlisten> {
  if (!isTauri) {
    return () => undefined;
  }

  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(eventName, (event) => handler(event.payload));
}

export function runningInTauri(): boolean {
  return isTauri;
}

export const bridgeApi = {
  startBridge(settings: BridgeSettings) {
    return invokeTauri<void>("start_bridge", { settings });
  },
  stopBridge() {
    return invokeTauri<void>("stop_bridge");
  },
  getStatus() {
    return invokeTauri<BridgeStatus>("get_status");
  },
  testGlympse(settings: BridgeSettings) {
    return invokeTauri<LocationFix[]>("test_glympse", { settings });
  },
  diagnoseGlympse(settings: BridgeSettings) {
    return invokeTauri<GlympseDiagnostics>("diagnose_glympse", { settings });
  },
  pollOnce(settings: BridgeSettings) {
    return invokeTauri<PollOutcome>("poll_once", { settings });
  },
  onLog(handler: (payload: BridgeLog) => void) {
    return listenTauri<BridgeLog>("bridge-log", handler);
  },
  onStatus(handler: (payload: BridgeStatus) => void) {
    return listenTauri<BridgeStatus>("bridge-status", handler);
  },
  onLocation(handler: (payload: LocationFix) => void) {
    return listenTauri<LocationFix>("location-updated", handler);
  },
  onLocations(handler: (payload: LocationFix[]) => void) {
    return listenTauri<LocationFix[]>("locations-updated", handler);
  },
  onForward(handler: (payload: ForwardEvent) => void) {
    return listenTauri<ForwardEvent>("forward-result", handler);
  }
};

async function demoInvoke<T>(command: string): Promise<T> {
  await new Promise((resolve) => window.setTimeout(resolve, 180));

  if (command === "get_status") {
    return { running: false, message: "Preview mode. Open the Tauri app for real network polling." } as T;
  }
  if (command === "test_glympse") {
    return demoLocations() as T;
  }
  if (command === "diagnose_glympse") {
    return {
      extractedCode: "!DEMO2026",
      codeVariants: ["!DEMO2026", "DEMO2026"],
      parsedLocation: demoLocations()[0],
      summary: "Preview diagnostics found 4 named demo users. Run the desktop app for real network polling.",
      attempts: [
        {
          url: "https://api.glympse.com/v2/groups/DEMO2026",
          status: 200,
          ok: true,
          contentType: "application/json",
          parsed: true,
          message: "Parsed named demo users from this preview response.",
          responsePreview: "{\"response\":{\"members\":\"redacted preview\"}}"
        }
      ]
    } as T;
  }
  if (command === "poll_once") {
    const locations = demoLocations();
    const now = Date.now();
    return {
      location: locations[0],
      locations,
      forward: demoForward(locations[0], "sent", now),
      forwards: locations.map((location, index) =>
        demoForward(location, index === 3 ? "skipped" : "sent", now - index * 9000)
      ),
      message: "Preview forwarded 3 demo users and skipped 1 unchanged fix."
    } as T;
  }
  if (command === "start_bridge") {
    throw new Error("Preview mode cannot start live polling. Run the Tauri desktop app for real Glympse and CalTopo calls.");
  }
  return undefined as T;
}

function demoLocations(): LocationFix[] {
  const timestampMs = Date.now() - 45_000;
  return [
    { lat: 37.44188, lng: -122.14302, accuracy: 9, altitude: 27, timestampMs, sourceLabel: "Net Control" },
    { lat: 37.38605, lng: -122.08385, accuracy: 12, altitude: 41, timestampMs, sourceLabel: "SAG 1" },
    { lat: 37.33182, lng: -122.03065, accuracy: 8, altitude: 76, timestampMs, sourceLabel: "Course Sweep" },
    { lat: 37.28411, lng: -122.17448, accuracy: 15, altitude: 318, timestampMs, sourceLabel: "Rest Stop 2" }
  ];
}

function demoForward(location: LocationFix, status: ForwardEvent["status"], timestampMs: number): ForwardEvent {
  const caltopoId = String(location.sourceLabel || "Demo").replace(/[^A-Za-z0-9]/g, "");
  return {
    caltopoId,
    sourceLabel: location.sourceLabel,
    lat: location.lat,
    lng: location.lng,
    status,
    message:
      status === "skipped"
        ? "Location unchanged since last forwarded fix"
        : "CalTopo accepted the preview fix with HTTP 200.",
    timestampMs
  };
}
