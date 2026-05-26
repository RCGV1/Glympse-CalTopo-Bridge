# Glympse CalTopo Bridge

Cross-platform Tauri desktop app that polls a live Glympse share and forwards each named participant to a matching CalTopo live track.

The bridge is meant for real group tracking: one Glympse group in, many CalTopo tracks out. Track IDs are derived automatically from each Glympse user or track name, so you do not manually assign IDs for every participant.

## What It Does

- Reads Glympse share links, invite codes, and public `!tag` sources through Glympse's anonymous viewer flow.
- Follows public Glympse group members and forwards each active named member separately.
- Derives every CalTopo track ID from the Glympse user or track name.
- Skips unnamed or generic parsed fixes instead of collapsing a group into one shared CalTopo track.
- Skips unchanged fixes by default so CalTopo is not spammed with duplicate reports.
- Redacts OAuth tokens, API keys, and passwords from diagnostics.
- Builds desktop bundles for macOS Apple Silicon, macOS Intel, Linux, and Windows.

## Quick Start

1. Download the latest desktop build from [Releases](https://github.com/RCGV1/Glympse-CalTopo-Bridge/releases/latest).
2. Create or copy a CalTopo live-track connect key.
3. Create or copy an active Glympse share URL, invite code, or public `!tag`.
4. Open the app, paste both values into **Bridge setup**, then click **Test Glympse**.
5. If named users appear, click **Forward once now** and verify the tracks in CalTopo.
6. Click **Start bridge** to keep syncing.

## Raspberry Pi OS Install

Raspberry Pi OS support is for 64-bit Pi OS on ARM64. On the Pi, run:

```bash
curl -fsSL https://raw.githubusercontent.com/RCGV1/Glympse-CalTopo-Bridge/main/scripts/install-raspi.sh | bash
```

The installer downloads the latest `*_arm64.deb` release asset and installs it with `apt`, including package dependencies.

Manual install:

```bash
sudo apt install ./Glympse.CalTopo.Bridge_0.1.4_arm64.deb
```

Use the `arm64.deb` file for 64-bit Raspberry Pi OS. The `amd64.deb` and `AppImage` files are for Intel/AMD Linux computers, not Raspberry Pi. The project does not currently publish a 32-bit `armhf` Raspberry Pi OS package.

## Tutorial

### 1. Prepare CalTopo

Create a CalTopo live-track custom system, team connect key, or equivalent live tracking connect key. The bridge sends reports to CalTopo with this request shape:

```text
https://caltopo.com/api/v1/position/report/{CONNECT_KEY}?id={TRACK_ID}&lat={LAT}&lng={LNG}
```

Copy only the connect key value into the app. Do not paste a full report URL unless the key itself is the only part you are using.

### 2. Prepare Glympse

Start an active Glympse from the mobile app, copy an invite/share link, or use a public Glympse tag such as `!GroupName`. The app accepts these source formats:

```text
https://glympse.com/!ABC123
!ABC123
ABC123
!PublicTag
```

For large groups, make sure every participant has a clear Glympse display name before starting the bridge. CalTopo IDs are made from those names by removing spaces, hyphens, and non-alphanumeric characters:

```text
Lead Vehicle -> LeadVehicle
Sweep Two    -> SweepTwo
Car-1        -> Car1
```

Names that become identical after cleanup will map to the same CalTopo track, so avoid near-duplicates such as `Car 1`, `Car-1`, and `Car_1` in the same group.

### 3. Test Before Forwarding

Open **Glympse CalTopo Bridge** and fill in:

- **Glympse share URL or invite code**: your Glympse link, invite code, or `!tag`.
- **CalTopo live-track connect key**: your CalTopo connect key.

Click **Diagnose source** first when you are not sure the Glympse source is readable. Diagnostics try the anonymous viewer login, source variants with and without `!`, group member lookups, and a short redacted response preview.

Click **Test Glympse** before sending anything to CalTopo. A good test shows active users in **Tracked Glympse users** with messages like:

```text
Forwarded as CalTopo track LeadVehicle
Forwarded as CalTopo track SweepTwo
```

If a row says it is not forwarded until Glympse provides a usable name, fix the participant's Glympse name or source before running the bridge.

### 4. Send One Fix To CalTopo

Click **Forward once now**. This performs one Glympse poll and one CalTopo send for each active named user.

Check **Activity**:

- `sent` means CalTopo accepted the position.
- `skipped` means the position matched the last sent fix and duplicate forwarding is disabled.
- `failed` means Glympse or CalTopo returned an error, or the fix did not include a usable name.

Then open CalTopo and confirm that each participant appears as a separate live track.

### 5. Run The Bridge

Click **Start bridge** after the one-shot test works. The status changes to `Running. Polling every Ns`.

Default behavior is conservative:

- Poll interval: 5 seconds.
- Unchanged fixes: not forwarded.
- Altitude: included when Glympse provides it.

Use **Advanced forwarding** when needed:

- Increase **Poll interval** for very large groups or slow networks.
- Enable **Send unchanged fixes too** only when you need repeated reports even without movement.
- Disable **Include altitude when available** if your CalTopo workflow expects only latitude and longitude.

Click **Stop** when the event or test is done.

## Large Group Checklist

- Use one active Glympse group or public tag that exposes member names.
- Give every participant a short, unique display name before the event starts.
- Avoid names that normalize to the same CalTopo ID.
- Run **Test Glympse** and confirm the expected participant count before forwarding.
- Run **Forward once now** and verify tracks in CalTopo before leaving the bridge unattended.
- Use a longer poll interval if the group has many participants.
- Keep the computer awake and online while the bridge is running.

## Troubleshooting

### Test Glympse Finds No Users

Make sure the Glympse share is currently active. Expired shares and empty public tags often return valid responses with no usable location. Click **Diagnose source** and inspect which attempted URL parsed, failed, or discovered group member lookups.

### A Participant Does Not Appear In CalTopo

Check **Tracked Glympse users**. If the participant is unnamed, the bridge intentionally does not forward that fix. Set a real Glympse display name and test again.

### Multiple People Collapse Into One CalTopo Track

Their names probably normalize to the same ID. Rename the Glympse users so the alphanumeric form is unique. For example, use `Lead1`, `Lead2`, and `Lead3` instead of `Lead 1`, `Lead-1`, and `Lead_1`.

### CalTopo Rejects A Send

Verify the connect key is correct and still active. The app only needs the connect key, not a full CalTopo report URL. The **Activity** panel shows the HTTP status and a short response summary when CalTopo rejects a request.

### The Same Point Is Not Sent Repeatedly

That is the default duplicate protection. Open **Advanced forwarding** and enable **Send unchanged fixes too** if your use case requires repeated reports with the same coordinates.

## Run Locally

Install Node.js and Rust, then run:

```bash
npm ci
npm run tauri:dev
```

Browser preview mode can render the UI, but real Glympse and CalTopo calls require the Tauri desktop app.

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

## Release

```bash
git tag vX.Y.Z
git push origin main --tags
```

The release workflow uses `tauri-apps/tauri-action` to create the GitHub release and upload desktop bundles.

## License

GPL-3.0-only. See `LICENSE`.
