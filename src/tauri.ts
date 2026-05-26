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
  if (command === "diagnose_glympse" || command === "test_glympse" || command === "poll_once" || command === "start_bridge") {
    throw new Error("Preview mode cannot contact Glympse or CalTopo. Run the desktop app to use real APIs.");
  }
  return undefined as T;
}
