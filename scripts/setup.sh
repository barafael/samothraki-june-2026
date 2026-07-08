#!/usr/bin/env bash
# Local preview of the static VIEWER build without R2.
#
# Builds the viewer with an empty ASSET_BASE_URL (root-relative) and copies the
# photos, transcodes, thumbs, and manifest next to the bundle so it all serves
# from one origin. Then serve `$BUNDLE` with any static file server.
#
# For editing metadata, don't use this — run the editor instead:
#   dx serve --features editor --fullstack
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Generating manifest.json + thumbnails"
cargo run --release -p photo-extract --bin gen-manifest

echo "==> Building viewer bundle (root-relative asset URLs)"
ASSET_BASE_URL="" dx build --release --platform web

BUNDLE="target/dx/my-holiday/release/web/public"

echo "==> Staging assets alongside the bundle"
mkdir -p "$BUNDLE/photos" "$BUNDLE/thumbs" "$BUNDLE/media"
cp -r Photos-3-001/. "$BUNDLE/photos/"
cp -r thumbs/.       "$BUNDLE/thumbs/"
[ -d media-web ] && cp -r media-web/. "$BUNDLE/media/" || true
cp manifest.json "$BUNDLE/manifest.json"

echo "Done. Serve the viewer with e.g.:"
echo "    python3 -m http.server -d $BUNDLE 8080"
