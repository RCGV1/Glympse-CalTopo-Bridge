const CLASSES = ["Shape", "AppTrack", "FieldTrack", "LiveTrack"];
const MARKER_CLASSES = ["Marker"];
const FOLDER_CLASSES = ["Folder"];
const MIN_POINTS = 2;
const FETCH_CONCURRENCY = 8;

export class PlaybackError extends Error {
  constructor(message, status = 500) {
    super(message);
    this.status = status;
  }
}

export default async function handler(request, response) {
  response.setHeader("Access-Control-Allow-Origin", "*");
  response.setHeader("Access-Control-Allow-Methods", "GET, OPTIONS");
  response.setHeader("Access-Control-Allow-Headers", "Accept, Content-Type");
  response.setHeader("Cache-Control", "no-store, max-age=0");
  response.setHeader("CDN-Cache-Control", "no-store");
  if (request.method === "OPTIONS") return response.status(204).end();
  try {
    const data = await getPlayback(request.query.url || request.query.map || "");
    return response.status(200).json(data);
  } catch (error) {
    return response.status(error instanceof PlaybackError ? error.status : 500).json({
      error: errorMessage(error)
    });
  }
}

export async function getPlayback(input) {
  const mapId = parseMapId(input);
  if (!mapId) throw new PlaybackError("Paste a CalTopo map URL or map ID.", 400);
  return exportMap(mapId);
}

export function errorMessage(error) {
  if (error instanceof PlaybackError) return error.message;
  return "CalTopo could not load this map. Verify the map link is public or share-link accessible and try again.";
}

export function parseMapId(input) {
  const value = String(input).trim();
  if (/^[A-Za-z0-9]{5,12}$/.test(value)) return value;
  try {
    const url = new URL(value);
    const match = url.pathname.match(/\/m\/([A-Za-z0-9]+)/);
    return match?.[1] || "";
  } catch {
    return "";
  }
}

async function exportMap(mapId) {
  const summary = await fetchJson(`https://caltopo.com/api/v1/map/${mapId}/since/0`, "map summary");
  const idsByClass = summary?.result?.ids || {};
  const tracks = [];
  const folders = new Map();
  const markers = [];
  const scanned = Object.fromEntries(
    [...FOLDER_CLASSES, ...CLASSES, ...MARKER_CLASSES].map((className) => [
      className,
      Array.isArray(idsByClass[className]) ? idsByClass[className].length : 0
    ])
  );

  const folderFeatures = await fetchMapObjects(mapId, idsByClass, FOLDER_CLASSES);
  for (const { feature, id } of folderFeatures) {
    const folder = normalizeFolder(feature, id);
    if (folder) folders.set(id, folder);
  }

  const [trackFeatures, markerFeatures] = await Promise.all([
    fetchMapObjects(mapId, idsByClass, CLASSES),
    fetchMapObjects(mapId, idsByClass, MARKER_CLASSES)
  ]);
  for (const { feature, className, id } of trackFeatures) {
    const track = normalizeTrack(feature, className, id);
    if (track) tracks.push(track);
  }
  for (const { feature, className, id } of markerFeatures) {
    const marker = normalizeMarker(feature, className, id, folders);
    if (marker) markers.push(marker);
  }

  tracks.sort((left, right) => left.start - right.start || left.title.localeCompare(right.title));
  markers.sort((left, right) => left.category.localeCompare(right.category) || left.title.localeCompare(right.title));
  if (tracks.length === 0) throw new Error(`No timestamped line tracks found on map ${mapId}.`);

  const start = Math.min(...tracks.map((track) => track.start));
  const end = Math.max(...tracks.map((track) => track.end));
  const bounds = tracks.reduce(
    (acc, track) => {
      for (const point of track.points) {
        acc.minLat = Math.min(acc.minLat, point.lat);
        acc.maxLat = Math.max(acc.maxLat, point.lat);
        acc.minLng = Math.min(acc.minLng, point.lng);
        acc.maxLng = Math.max(acc.maxLng, point.lng);
      }
      return acc;
    },
    { minLat: Infinity, maxLat: -Infinity, minLng: Infinity, maxLng: -Infinity }
  );
  for (const marker of markers) {
    bounds.minLat = Math.min(bounds.minLat, marker.lat);
    bounds.maxLat = Math.max(bounds.maxLat, marker.lat);
    bounds.minLng = Math.min(bounds.minLng, marker.lng);
    bounds.maxLng = Math.max(bounds.maxLng, marker.lng);
  }

  return {
    mapId,
    generatedAt: new Date().toISOString(),
    scanned,
    start,
    end,
    bounds,
    tracks,
    folders: [...folders.values()].sort((left, right) => left.title.localeCompare(right.title)),
    markers,
    markerGroups: markerGroups(markers)
  };
}

async function fetchMapObjects(mapId, idsByClass, classNames) {
  const objects = classNames.flatMap((className) =>
    (Array.isArray(idsByClass[className]) ? idsByClass[className] : []).map((id) => ({
      className,
      id: String(id)
    }))
  );
  return mapWithConcurrency(objects, FETCH_CONCURRENCY, async ({ className, id }) => ({
    className,
    id,
    feature: (await fetchJson(
      `https://caltopo.com/api/v1/map/${mapId}/${className}/${encodeURIComponent(id)}`,
      `${className} ${id}`
    ))?.result
  }));
}

async function mapWithConcurrency(items, limit, mapper) {
  const results = new Array(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const index = next++;
      results[index] = await mapper(items[index]);
    }
  };
  await Promise.all(Array.from({ length: Math.min(items.length, limit) }, worker));
  return results;
}

async function fetchJson(url, description) {
  let response;
  try {
    response = await fetch(url, { headers: { Accept: "application/json" } });
  } catch {
    throw new PlaybackError(`CalTopo could not be reached while loading ${description}.`, 502);
  }
  const text = await response.text();
  if (!response.ok) {
    const message = response.status === 401 || response.status === 403
      ? "CalTopo denied access to this map. Verify the shared map link."
      : `CalTopo could not load ${description} (HTTP ${response.status}).`;
    throw new PlaybackError(message, response.status >= 400 && response.status < 500 ? response.status : 502);
  }
  try {
    return text ? JSON.parse(text) : null;
  } catch {
    throw new PlaybackError(`CalTopo returned an invalid response while loading ${description}.`, 502);
  }
}

function normalizeTrack(feature, className, id) {
  const geometry = feature?.geometry;
  const properties = feature?.properties || {};
  if (geometry?.type !== "LineString") return null;
  const points = (geometry.coordinates || [])
    .map(timestampedPoint)
    .filter(Boolean)
    .sort((left, right) => left.time - right.time);
  if (points.length < MIN_POINTS) return null;
  const first = points[0];
  const last = points[points.length - 1];
  return {
    id,
    className,
    title: properties.title || properties.name || properties.deviceId || `${className} ${id}`,
    deviceId: properties.deviceId || "",
    color: normalizeColor(properties.stroke || properties.color) || colorFor(id),
    pointCount: points.length,
    start: first.time,
    end: last.time,
    points
  };
}

function normalizeFolder(feature, id) {
  const properties = feature?.properties || {};
  return {
    id,
    title: cleanText(properties.title || properties.name || `Folder ${id}`),
    visible: properties.visible !== false
  };
}

function normalizeMarker(feature, className, id, folders) {
  const geometry = feature?.geometry;
  const properties = feature?.properties || {};
  if (geometry?.type !== "Point") return null;
  const coord = Array.isArray(geometry.coordinates) ? geometry.coordinates : [];
  const [lng, lat, ele] = coord;
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  if (Math.abs(lat) > 90 || Math.abs(lng) > 180) return null;
  const folderId = properties.folderId || "";
  const folder = folderId ? folders.get(folderId) : null;
  const title = cleanText(properties.title || properties.name || `${className} ${id}`);
  const description = cleanText(properties.description || "");
  const symbol = cleanText(properties["marker-symbol"] || properties.symbol || "");
  const color = normalizeColor(properties["marker-color"] || properties.color || "");
  const category = markerCategory({ title, description, symbol, folderTitle: folder?.title || "" });
  const createdOn = markerCreatedTime(properties);
  if (!createdOn) return null;
  return {
    id,
    className,
    title,
    description,
    folderId,
    folderTitle: folder?.title || "",
    category,
    symbol,
    color: color || colorForMarkerCategory(category),
    size: cleanText(properties["marker-size"] || ""),
    labelVisible: Boolean(properties.labelVisible),
    lng: Number(lng.toFixed(7)),
    lat: Number(lat.toFixed(7)),
    ele: Number.isFinite(ele) ? Math.round(ele) : null,
    time: createdOn
  };
}

function markerCreatedTime(properties) {
  const value = properties["-created-on"];
  return Number.isFinite(value) && value > 0 ? Math.round(value) : null;
}

function markerGroups(markers) {
  const counts = new Map();
  for (const marker of markers) counts.set(marker.category, (counts.get(marker.category) || 0) + 1);
  return [...counts.entries()]
    .sort((left, right) => left[0].localeCompare(right[0]))
    .map(([name, count]) => ({ name, count }));
}

function markerCategory(marker) {
  const folder = marker.folderTitle.toLowerCase();
  if (/(hazard|gravel|restricted|dead zone)/.test(folder)) return "Hazards";
  if (/(rest stop|\brest\b|\brs\b|refresh)/.test(folder)) return "Rest Stops";
  if (/(checkpoint|monitor)/.test(folder)) return "Checkpoints";
  if (/(water|restroom|bathroom|shop|services|cell)/.test(folder)) return "Services";
  if (/(hq|start|finish|command)/.test(folder)) return "HQ";
  const haystack = `${marker.folderTitle} ${marker.title} ${marker.description} ${marker.symbol}`.toLowerCase();
  if (/(hazard|construction|closed|closure|danger|crash|downed|down rider|medical|incident|flat tire|one[ -]?way|confusing|restricted|unauthorized)/.test(haystack)) return "Hazards";
  if (/(rest stop|\(rs\)|\brs\b|refresh|foodservice|aid station)/.test(haystack)) return "Rest Stops";
  if (/(hq|start|finish|staging|command)/.test(haystack)) return "HQ";
  if (/(checkpoint|monitor|\(cp\)|\bcp\b|binoc)/.test(haystack)) return "Checkpoints";
  if (/(water|restroom|bathroom|toilet|shop|services|store|hut|lodging)/.test(haystack)) return "Services";
  return marker.folderTitle || "Other Markers";
}

function colorForMarkerCategory(category) {
  if (category === "Hazards") return "#d21f1f";
  if (category === "Rest Stops") return "#0f7b4f";
  if (category === "Checkpoints") return "#111827";
  if (category === "Services") return "#6a1b9a";
  if (category === "HQ") return "#0b6f6a";
  return "#475569";
}

function normalizeColor(value) {
  const text = String(value || "").trim();
  const match = text.match(/^#?([0-9a-f]{6})$/i);
  return match ? `#${match[1]}` : "";
}

function cleanText(value) {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function timestampedPoint(coord) {
  if (!Array.isArray(coord) || coord.length < 4) return null;
  const [lng, lat, ele, time] = coord;
  if (!Number.isFinite(lng) || !Number.isFinite(lat) || !Number.isFinite(time)) return null;
  if (Math.abs(lat) > 90 || Math.abs(lng) > 180) return null;
  return {
    lng: Number(lng.toFixed(7)),
    lat: Number(lat.toFixed(7)),
    ele: Number.isFinite(ele) ? Math.round(ele) : null,
    time: Math.round(time)
  };
}

function colorFor(value) {
  let hash = 0;
  for (const char of String(value)) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360}, 70%, 42%)`;
}
