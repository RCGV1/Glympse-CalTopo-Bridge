#!/usr/bin/env bash
set -euo pipefail

REPO="${REPO:-RCGV1/Glympse-CalTopo-Bridge}"
VERSION="${VERSION:-latest}"

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need_command curl
need_command python3
need_command dpkg
need_command apt-get

arch="$(dpkg --print-architecture)"
case "$arch" in
  arm64)
    ;;
  armhf)
    echo "This installer needs 64-bit Raspberry Pi OS." >&2
    echo "Current architecture is armhf, and this project does not publish a 32-bit ARM package." >&2
    exit 1
    ;;
  *)
    echo "This installer is for 64-bit Raspberry Pi OS only. Current architecture: $arch" >&2
    exit 1
    ;;
esac

if [[ "$(id -u)" -eq 0 ]]; then
  sudo_cmd=()
else
  need_command sudo
  sudo_cmd=(sudo)
fi

if [[ "$VERSION" == "latest" ]]; then
  api_url="https://api.github.com/repos/${REPO}/releases/latest"
else
  api_url="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
fi

echo "Looking up ${REPO} ${VERSION} for Raspberry Pi OS arm64..."
release_json="$(curl -fsSL "$api_url")"

asset_url="$(
  RELEASE_JSON="$release_json" python3 - <<'PY'
import json
import os
import sys

release = json.loads(os.environ["RELEASE_JSON"])
for asset in release.get("assets", []):
    name = asset.get("name", "")
    if name.endswith("_arm64.deb"):
        print(asset.get("browser_download_url", ""))
        break
else:
    tag = release.get("tag_name", "selected release")
    print(f"No Raspberry Pi OS arm64 .deb asset found in {tag}.", file=sys.stderr)
    sys.exit(1)
PY
)"

asset_name="${asset_url##*/}"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

deb_path="${tmp_dir}/${asset_name}"
echo "Downloading ${asset_name}..."
curl -fL "$asset_url" -o "$deb_path"

echo "Installing ${asset_name} with apt..."
"${sudo_cmd[@]}" apt-get update
"${sudo_cmd[@]}" apt-get install -y "$deb_path"

echo "Installed Glympse CalTopo Bridge."
echo "Launch it from the desktop menu or run: glympse-caltopo-bridge"
