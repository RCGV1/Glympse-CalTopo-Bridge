# Repository Instructions

This repository contains a Tauri 2 desktop app with a React/Vite frontend and a Rust backend. The app polls a Glympse source and forwards live positions to CalTopo live tracking.

## Working Rules

- Push assigned tasks to completion. If the requested outcome is not done, keep working until it is complete or a concrete external blocker is reached.
- Keep generated artifacts out of source control. Do not commit `node_modules/`, `dist/`, `src-tauri/target/`, `.DS_Store`, or `test-results/`.
- Preserve the bridge's safety behavior: do not synthesize fake coordinates, do not forward without an explicit CalTopo connect key, and redact tokens or API keys from diagnostics.
- Prefer small, focused changes that match the current React and Rust structure.
- Use ASCII in source files unless an existing file already requires non-ASCII text.

## Project Layout

- `src/` contains the React UI, Tauri API wrapper, shared TypeScript types, and frontend unit tests.
- `src-tauri/src/lib.rs` contains Tauri commands, Glympse parsing, CalTopo forwarding, diagnostics, and Rust unit tests.
- `src-tauri/tauri.conf.json` contains desktop window and bundle configuration.
- `.github/workflows/ci.yml` runs tests and frontend build on GitHub.
- `.github/workflows/release.yml` builds and publishes macOS, Linux, and Windows release bundles from version tags.

## Local Commands

- `npm ci` installs frontend and Tauri CLI dependencies.
- `npm run dev` starts the Vite frontend preview.
- `npm run tauri:dev` starts the desktop app.
- `npm run test:frontend` runs Vitest frontend unit tests.
- `npm run test:rust` runs Rust unit tests through Cargo.
- `npm test` runs the full unit test suite.
- `npm run build` type-checks and builds the frontend.
- `npm run tauri:build` builds the local platform desktop bundle.

## Release Process

1. Keep `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` on the same version.
2. Run `npm test` and `npm run build` before tagging.
3. Commit all source changes.
4. Create and push a tag like `v0.1.0`.
5. The release workflow publishes GitHub release assets for macOS Apple Silicon, macOS Intel, Linux, and Windows.

## Licensing

The project is GPL-3.0-only. Keep `LICENSE`, `package.json`, and `src-tauri/Cargo.toml` aligned when changing license metadata.
