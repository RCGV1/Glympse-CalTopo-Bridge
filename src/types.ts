export type BridgeSettings = {
  glympseSource: string;
  caltopoConnectKey: string;
  pollIntervalSecs: number;
  maxFixAgeSecs: number;
  forwardUnchanged: boolean;
  includeAltitude: boolean;
};

export type LocationFix = {
  lat: number;
  lng: number;
  accuracy?: number | null;
  altitude?: number | null;
  speed?: number | null;
  heading?: number | null;
  timestampMs?: number | null;
  sourceLabel?: string | null;
};

export type BridgeStatus = {
  running: boolean;
  message: string;
};

export type BridgeLog = {
  level: "info" | "warn" | "error" | "success";
  message: string;
  timestampMs: number;
};

export type ForwardEvent = {
  caltopoId: string;
  sourceLabel?: string | null;
  lat: number;
  lng: number;
  status: "sent" | "skipped" | "failed";
  message: string;
  timestampMs: number;
};

export type PollOutcome = {
  location?: LocationFix | null;
  forward?: ForwardEvent | null;
  locations?: LocationFix[];
  forwards?: ForwardEvent[];
  message: string;
};

export type GlympseDiagnostics = {
  extractedCode?: string | null;
  codeVariants: string[];
  attempts: GlympseAttempt[];
  parsedLocation?: LocationFix | null;
  summary: string;
};

export type GlympseAttempt = {
  url: string;
  status?: number | null;
  ok: boolean;
  contentType?: string | null;
  parsed: boolean;
  message: string;
  responsePreview?: string | null;
};
