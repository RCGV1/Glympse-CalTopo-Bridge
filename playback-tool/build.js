import { accessSync, constants, readFileSync } from "node:fs";
import { resolve } from "node:path";

const publicHtml = resolve("public/index.html");
for (const file of [publicHtml, resolve("api/playback.js"), resolve("worker.js")]) {
  accessSync(file, constants.R_OK);
}

const html = readFileSync(publicHtml, "utf8");
if (!html.includes('id="map"') || !html.includes("/api/playback")) {
  throw new Error("public/index.html is missing the standalone playback application.");
}

console.log("Playback tool is ready for deployment.");
