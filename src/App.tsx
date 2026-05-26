import { useEffect, useState } from "react";
import { Activity, CheckCircle2, CircleAlert, Eraser, MapPin, Pause, Play, Send } from "lucide-react";
import {
  buildActivityItems,
  buildCaltopoIdPreview,
  buildMissingFields,
  buildPreflightChecks,
  formatCoordinates,
  formatForwardName,
  formatList,
  formatLocationMeta,
  formatTime,
  formatTrackedUsers,
  normalizePollInterval,
  stringifyError,
  titleCase
} from "./bridgeUtils";
import { bridgeApi, runningInTauri } from "./tauri";
import type { BridgeLog, BridgeSettings, BridgeStatus, ForwardEvent, GlympseDiagnostics, LocationFix } from "./types";

const SETTINGS_KEY = "glympse-caltopo-bridge.settings.v1";

const defaultSettings: BridgeSettings = {
  glympseSource: "",
  caltopoConnectKey: "",
  caltopoDeviceId: "",
  pollIntervalSecs: 5,
  forwardUnchanged: false,
  includeAltitude: true
};

function loadSettings(): BridgeSettings {
  const stored = window.localStorage.getItem(SETTINGS_KEY);
  if (!stored) return defaultSettings;

  try {
    const parsed = JSON.parse(stored) as Partial<BridgeSettings>;
    return {
      ...defaultSettings,
      ...parsed,
      pollIntervalSecs: normalizePollInterval(parsed.pollIntervalSecs ?? defaultSettings.pollIntervalSecs)
    };
  } catch {
    return defaultSettings;
  }
}

export function App() {
  const [settings, setSettings] = useState<BridgeSettings>(() => loadSettings());
  const [status, setStatus] = useState<BridgeStatus>({ running: false, message: "Stopped" });
  const [latestLocations, setLatestLocations] = useState<LocationFix[]>([]);
  const [diagnostics, setDiagnostics] = useState<GlympseDiagnostics | null>(null);
  const [forwards, setForwards] = useState<ForwardEvent[]>([]);
  const [logs, setLogs] = useState<BridgeLog[]>([
    {
      level: "info",
      message: runningInTauri()
        ? "Ready. Add a Glympse share and CalTopo live-track key."
        : "Browser preview mode. Run the Tauri desktop app for real Glympse and CalTopo calls.",
      timestampMs: Date.now()
    }
  ]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    window.localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    let mounted = true;
    const unlisteners: Array<() => void> = [];

    async function boot() {
      try {
        const initialStatus = await bridgeApi.getStatus();
        if (!mounted) return;
        setStatus(initialStatus);

        unlisteners.push(await bridgeApi.onStatus((payload) => setStatus(payload)));
        unlisteners.push(await bridgeApi.onLocation((payload) => setLatestLocations([payload])));
        unlisteners.push(await bridgeApi.onLocations((payload) => setLatestLocations(payload)));
        unlisteners.push(
          await bridgeApi.onForward((payload) =>
            setForwards((current) => [payload, ...current].slice(0, 80))
          )
        );
        unlisteners.push(
          await bridgeApi.onLog((payload) =>
            setLogs((current) => [payload, ...current].slice(0, 200))
          )
        );
      } catch (error) {
        pushLocalLog("error", stringifyError(error));
      }
    }

    void boot();
    return () => {
      mounted = false;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);

  const missingFields = buildMissingFields(settings);
  const canRun = missingFields.length === 0 && !busy;
  const lastForward = forwards[0] ?? null;
  const activityItems = buildActivityItems(forwards, logs);
  const latestLocation = latestLocations[0] ?? null;
  const caltopoIdPreview = buildCaltopoIdPreview(settings, latestLocation);
  const preflightChecks = buildPreflightChecks(settings, latestLocation);

  function updateSetting<K extends keyof BridgeSettings>(key: K, value: BridgeSettings[K]) {
    setSettings((current) => ({ ...current, [key]: value }));
  }

  function pushLocalLog(level: BridgeLog["level"], message: string) {
    setLogs((current) => [{ level, message, timestampMs: Date.now() }, ...current].slice(0, 200));
  }

  async function start() {
    setBusy(true);
    try {
      await bridgeApi.startBridge(settings);
    } catch (error) {
      pushLocalLog("error", stringifyError(error));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      await bridgeApi.stopBridge();
    } catch (error) {
      pushLocalLog("error", stringifyError(error));
    } finally {
      setBusy(false);
    }
  }

  async function testGlympse() {
    setBusy(true);
    try {
      const locations = await bridgeApi.testGlympse(settings);
      setLatestLocations(locations);
      pushLocalLog(
        "success",
        `Read ${locations.length} active Glympse user${locations.length === 1 ? "" : "s"} without forwarding.`
      );
    } catch (error) {
      pushLocalLog("error", stringifyError(error));
    } finally {
      setBusy(false);
    }
  }

  async function diagnoseGlympse() {
    setBusy(true);
    try {
      const report = await bridgeApi.diagnoseGlympse(settings);
      setDiagnostics(report);
      if (report.parsedLocation) setLatestLocations([report.parsedLocation]);
      pushLocalLog(report.parsedLocation ? "success" : "warn", report.summary);
    } catch (error) {
      pushLocalLog("error", stringifyError(error));
    } finally {
      setBusy(false);
    }
  }

  async function forwardOnce() {
    setBusy(true);
    try {
      const outcome = await bridgeApi.pollOnce(settings);
      if (outcome.locations?.length) setLatestLocations(outcome.locations);
      else if (outcome.location) setLatestLocations([outcome.location]);
    } catch (error) {
      pushLocalLog("error", stringifyError(error));
    } finally {
      setBusy(false);
    }
  }

  function clearActivity() {
    setDiagnostics(null);
    setForwards([]);
    setLogs([]);
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div>
          <h1>Live location bridge</h1>
          <p>Glympse users to matching CalTopo live tracks.</p>
        </div>
        <div className="top-actions">
          <span className={`status-text ${status.running ? "running" : ""}`}>{status.message}</span>
          {status.running ? (
            <button className="primary danger" onClick={stop} disabled={busy}>
              <Pause size={18} />
              Stop
            </button>
          ) : (
            <button className="primary" onClick={start} disabled={!canRun}>
              <Play size={18} />
              Start bridge
            </button>
          )}
        </div>
      </header>

      <main className="layout">
        <section className="setup-panel">
          <h2 className="panel-title">Bridge setup</h2>

          <label className="field">
            <span>Glympse share URL or invite code</span>
            <textarea
              value={settings.glympseSource}
              onChange={(event) => updateSetting("glympseSource", event.target.value)}
              placeholder="https://glympse.com/!ABC123 or a raw invite code"
              rows={3}
            />
          </label>

          <div className="field-grid">
            <label className="field">
              <span>CalTopo live-track connect key</span>
              <input
                value={settings.caltopoConnectKey}
                onChange={(event) => updateSetting("caltopoConnectKey", event.target.value)}
                placeholder="Connect key"
                autoCapitalize="none"
                spellCheck={false}
              />
            </label>

            <label className="field">
              <span>
                CalTopo ID fallback <small>optional</small>
              </span>
              <input
                value={settings.caltopoDeviceId}
                onChange={(event) => updateSetting("caltopoDeviceId", event.target.value)}
                placeholder="Fallback track ID"
                autoCapitalize="none"
                spellCheck={false}
              />
            </label>
          </div>

          <p className="setup-hint compact">
            Next CalTopo track ID: <strong>{caltopoIdPreview.value}</strong> ({caltopoIdPreview.label}).
          </p>

          <details className="advanced-options">
            <summary>Advanced forwarding</summary>
            <div className="field-grid compact">
              <label className="field">
                <span>Poll interval</span>
                <div className="number-input">
                  <input
                    type="number"
                    min={2}
                    value={settings.pollIntervalSecs}
                    onChange={(event) =>
                      updateSetting("pollIntervalSecs", normalizePollInterval(event.target.value))
                    }
                  />
                  <span>seconds</span>
                </div>
              </label>
              <div className="field options">
                <span>Forwarding</span>
                <label>
                  <input
                    type="checkbox"
                    checked={settings.forwardUnchanged}
                    onChange={(event) => updateSetting("forwardUnchanged", event.target.checked)}
                  />
                  Send unchanged fixes too
                </label>
                <label>
                  <input
                    type="checkbox"
                    checked={settings.includeAltitude}
                    onChange={(event) => updateSetting("includeAltitude", event.target.checked)}
                  />
                  Include altitude when available
                </label>
              </div>
            </div>
          </details>

          <div className="button-row">
            <button onClick={diagnoseGlympse} disabled={busy || !settings.glympseSource.trim()}>
              <Activity size={17} />
              Diagnose source
            </button>
            <button onClick={testGlympse} disabled={busy || !settings.glympseSource.trim()}>
              <MapPin size={17} />
              Test Glympse
            </button>
            <button onClick={forwardOnce} disabled={!canRun}>
              <Send size={17} />
              Forward once now
            </button>
            <button onClick={clearActivity} disabled={busy || (!logs.length && !forwards.length && !diagnostics)}>
              <Eraser size={17} />
              Clear activity
            </button>
          </div>

          <div className="preflight-block">
            <h3>Preflight</h3>
            <ul className="preflight-list">
              {preflightChecks.map((check) => (
                <li className={check.ok ? "ok" : "missing"} key={check.label}>
                  {check.ok ? <CheckCircle2 size={16} /> : <CircleAlert size={16} />}
                  <span>{check.label}</span>
                </li>
              ))}
            </ul>
          </div>

          {missingFields.length > 0 ? (
            <p className="setup-hint">Add {formatList(missingFields)} before starting.</p>
          ) : (
            <p className="setup-hint ready">Ready to start. Test the source first if this is a new link.</p>
          )}

          {diagnostics ? (
            <div className="diagnostics-panel">
              <div className="diagnostics-head">
                <h3>Diagnostics</h3>
                <p>{diagnostics.summary}</p>
              </div>
              <details className="technical-details">
                <summary>Technical details</summary>
                <div className="diagnostics-meta">
                  <span>Code: {diagnostics.extractedCode || "none"}</span>
                  <span>
                    Variants: {diagnostics.codeVariants.length ? diagnostics.codeVariants.join(", ") : "none"}
                  </span>
                </div>
                <div className="attempt-list">
                  {diagnostics.attempts.map((attempt) => (
                    <details className="attempt-item" key={attempt.url}>
                      <summary>
                        <strong>{attempt.parsed ? "Parsed" : attempt.status ?? "Error"}</strong>
                        <span>{attempt.url}</span>
                      </summary>
                      <p>{attempt.message}</p>
                      {attempt.contentType ? <small>Content-Type: {attempt.contentType}</small> : null}
                      {attempt.responsePreview ? <pre>{attempt.responsePreview}</pre> : null}
                    </details>
                  ))}
                </div>
              </details>
            </div>
          ) : null}
        </section>

        <section className="operations-panel">
          <div className="current-panel">
            <h2>Current status</h2>
            <dl>
              <div>
                <dt>Latest Glympse fix</dt>
                <dd>{latestLocation ? formatCoordinates(latestLocation) : "No fix read yet"}</dd>
              </div>
              <div>
                <dt>Tracked users</dt>
                <dd>{latestLocations.length ? formatTrackedUsers(latestLocations) : "No users read yet"}</dd>
              </div>
              <div>
                <dt>Fix detail</dt>
                <dd>{latestLocation ? formatLocationMeta(latestLocation) : "Use Test Glympse to check the source"}</dd>
              </div>
              <div>
                <dt>Last CalTopo send</dt>
                <dd className={lastForward ? `status-${lastForward.status}` : ""}>
                  {lastForward ? `${titleCase(lastForward.status)} at ${formatTime(lastForward.timestampMs)}` : "Nothing sent yet"}
                </dd>
              </div>
              <div>
                <dt>Successful sends</dt>
                <dd>{forwards.filter((event) => event.status === "sent").length}</dd>
              </div>
            </dl>
          </div>

          {latestLocations.length > 0 ? (
            <div className="detail-panel tracked-panel">
              <h2 className="panel-title">Tracked Glympse users</h2>
              <div className="tracked-list">
                {latestLocations.map((location) => {
                  const name = location.sourceLabel || "Unnamed Glympse user";
                  return (
                    <div className="tracked-item" key={`${name}-${location.lat}-${location.lng}`}>
                      <div>
                        <strong>{name}</strong>
                        <span>Forwarded as its own CalTopo live track</span>
                      </div>
                      <span>{formatCoordinates(location)}</span>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}

          <div className="detail-panel">
            <h2 className="panel-title">Activity</h2>
            <div className="activity-list">
              {activityItems.length === 0 ? (
                <div className="empty-state">No activity yet.</div>
              ) : (
                activityItems.map((item) => (
                  item.kind === "forward" ? (
                    <div className="history-item" key={`forward-${item.event.timestampMs}-${item.event.status}`}>
                      <div>
                        <strong>{formatForwardName(item.event)}</strong>
                        <span>{formatCoordinates(item.event)}</span>
                      </div>
                      <div>
                        <span className={`plain-status ${item.event.status}`}>{item.event.status}</span>
                        <small>{formatTime(item.event.timestampMs)}</small>
                      </div>
                      <p>{item.event.message}</p>
                    </div>
                  ) : (
                    <div className={`log-item ${item.log.level}`} key={`log-${item.log.timestampMs}-${item.log.message}`}>
                      <span>{formatTime(item.log.timestampMs)}</span>
                      <p>{item.log.message}</p>
                    </div>
                  )
                ))
              )}
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
