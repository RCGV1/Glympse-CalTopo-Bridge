import { errorMessage, getPlayback, PlaybackError } from "./api/playback.js";

const NO_STORE_HEADERS = {
  "Content-Type": "application/json; charset=utf-8",
  "Cache-Control": "no-store, max-age=0",
  "CDN-Cache-Control": "no-store"
};

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api/playback") return playbackResponse(url);

    if (env.ASSETS) return env.ASSETS.fetch(request);
    return new Response("Static assets are not configured. Deploy with --assets=public.", {
      status: 500,
      headers: { "Cache-Control": "no-store" }
    });
  }
};

async function playbackResponse(url) {
  try {
    const data = await getPlayback(url.searchParams.get("url") || url.searchParams.get("map") || "");
    return json(data, 200);
  } catch (error) {
    return json(
      { error: errorMessage(error) },
      error instanceof PlaybackError ? error.status : 500
    );
  }
}

function json(body, status) {
  return new Response(JSON.stringify(body), { status, headers: NO_STORE_HEADERS });
}
