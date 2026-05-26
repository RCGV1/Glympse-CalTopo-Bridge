# Glympse CalTopo Bridge

Cross-platform Tauri desktop app that polls a Glympse share and forwards the newest real positions to CalTopo live tracking.

## What It Does

- Reads Glympse share links, invite codes, and public `!tag` sources through Glympse's anonymous viewer flow.
- Parses Glympse ticket streams, tagged-map group members, embedded coordinate payloads, and common JSON coordinate shapes.
- Sends each active Glympse user to CalTopo as a separate live track when Glympse provides a display name.
- Falls back to a manual CalTopo track ID, then to `Glympse`, when a source does not expose a usable user name.
- Skips unchanged fixes by default so CalTopo is not spammed with duplicate reports.
- Redacts OAuth tokens, API keys, and passwords from diagnostics.

## Run Locally

```bash
npm ci
npm run tauri:dev
```

## Test

```bash
npm test
npm run build
```

`npm test` runs Vitest frontend unit tests and Rust unit tests in `src-tauri`.

## Build

```bash
npm run tauri:build
```

The local Tauri build produces a bundle for the current platform. GitHub Actions builds release bundles for macOS Apple Silicon, macOS Intel, Linux, and Windows when a `v*.*.*` tag is pushed.

## CalTopo Setup

Create a CalTopo live-track custom system or team connect key. The bridge sends reports in this shape:

```text
https://caltopo.com/api/v1/position/report/{CONNECT_KEY}?id={TRACK_ID}&lat={LAT}&lng={LNG}
```

Enter the CalTopo connect key in the app. The app derives `TRACK_ID` from each Glympse display name when possible and strips spaces, hyphens, and other non-alphanumeric characters because those IDs bind more reliably in CalTopo.

Use the optional CalTopo ID fallback when a source has no usable Glympse display name.

## Glympse Setup

Paste either a full Glympse share link or a raw invite/share code. Use `Diagnose source` when a link does not work. Diagnostics check anonymous viewer login, try source variants with and without `!`, follow active member invite codes from public tags, and show a short redacted response preview for each attempted URL.

For a true live test, create an active Glympse from the mobile app or join a public tag, copy that active `https://glympse.com/...` link into the app, then click `Test Glympse`.

## Release

```bash
git tag v0.1.0
git push origin main --tags
```

The release workflow uses `tauri-apps/tauri-action` to create the GitHub release and upload desktop bundles.

## License

GPL-3.0-only. See `LICENSE`.
