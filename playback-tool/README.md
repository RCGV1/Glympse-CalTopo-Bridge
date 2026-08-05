# CalTopo Playback Tool

Replay timestamped CalTopo tracks and event markers from a public or share-link-accessible map.

The hosted tool is available at [caltopo-track-playback.vercel.app](https://caltopo-track-playback.vercel.app/).

## What It Does

- Loads CalTopo map links, share links, and raw map IDs.
- Replays timestamped `Shape`, `AppTrack`, `FieldTrack`, and `LiveTrack` line data.
- Shows CalTopo markers such as hazards, rest stops, checkpoints, services, and HQ.
- Replays marker additions using each marker's CalTopo created time.
- Supports trail playback, position-only playback, stale-track fadeout, range selection, and per-track visibility.
- Keeps marker labels hover-only by default to reduce clutter.
- Uses a collapsed bottom list on mobile so the map remains the primary view.

## Quick Start

1. Open [caltopo-track-playback.vercel.app](https://caltopo-track-playback.vercel.app/).
2. Paste a CalTopo map URL, share URL, or map ID.
3. Select **Load**.
4. Use **Play**, the timeline slider, speed, stale timeout, view mode, and range controls to review the event.
5. Toggle tracks or marker groups in the side panel. On phones, tap **List** to open the drawer.

You can share a preloaded playback URL by adding the map as a query parameter:

```text
https://caltopo-track-playback.vercel.app/?map=https%3A%2F%2Fcaltopo.com%2Fm%2FYOURMAPID
```

## Map Requirements

- The CalTopo map must be public or accessible through the provided share link.
- Tracks must contain timestamped line coordinates.
- Markers are optional. When present, they are grouped from CalTopo folders and marker metadata.
- The tool does not need or store CalTopo account credentials.

## Marker Categories

Marker groups are inferred from folder names, marker names, descriptions, and symbols:

- Hazards
- Rest Stops
- Checkpoints
- Services
- HQ
- Other Markers

Markers created before the replay window are visible at the start. Markers created during the event appear when playback reaches their CalTopo created time.

## Deploying

This folder is a standalone Vercel app:

```sh
npm install
npm run build
vercel deploy --prod
```

The serverless API fetches CalTopo map data from the server side. This avoids browser CORS limitations when loading arbitrary CalTopo maps. Playback responses use `Cache-Control: no-store`, so map data is not cached by the tool or its CDN.

## Cloudflare Worker Deployment

`worker.js` imports the same API normalizer as Vercel. Deploy it with the static page as Worker Assets:

```sh
npm run build
npx wrangler deploy worker.js --assets=public
```
