#!/usr/bin/env bash
# Publish the static viewer to Cloudflare (R2 assets + Pages bundle).
#
# Pipeline:
#   1. transcode HEVC videos -> media-web/ (browser-playable H.264)
#   2. generate manifest.json + WebP thumbnails from the curated photo_data.json
#   3. sync photos, transcodes, thumbs, and manifest to the R2 bucket
#   4. build the viewer (pure dioxus/web, no fullstack) with ASSET_BASE_URL
#      pointing at the R2 custom domain
#   5. (optional) deploy the static bundle to Cloudflare Pages
#
# Local disk (Photos-3-001/) is the source of truth; R2 is the published copy.
#
# Required env:
#   ASSET_BASE_URL   public URL assets are served from, e.g.
#                    https://assets.samothraki.example  (no trailing slash)
#   R2_BUCKET        rclone remote:bucket, e.g. r2:samothraki-holiday
# Optional env:
#   CF_PAGES_PROJECT wrangler Pages project name; if set, step 5 deploys.
set -euo pipefail

cd "$(dirname "$0")/.."

: "${ASSET_BASE_URL:?set ASSET_BASE_URL to the public R2 asset URL}"
: "${R2_BUCKET:?set R2_BUCKET to the rclone remote:bucket}"

echo "==> 1/5 Transcoding videos (HEVC -> H.264)"
scripts/transcode_videos.sh

echo "==> 2/5 Generating manifest.json + thumbnails"
cargo run --release -p photo-extract --bin gen-manifest

echo "==> 3/5 Syncing assets to R2 ($R2_BUCKET)"
# Photos and transcodes are large and immutable; --size-only avoids re-hashing
# gigabytes on every publish. Thumbs/manifest are small and change often.
rclone sync --size-only Photos-3-001 "$R2_BUCKET/photos"
rclone sync --size-only media-web    "$R2_BUCKET/media"
rclone sync             thumbs       "$R2_BUCKET/thumbs"
rclone copyto           manifest.json "$R2_BUCKET/manifest.json"

echo "==> 4/5 Building viewer bundle (ASSET_BASE_URL=$ASSET_BASE_URL)"
# Pure web build: default features = ["web"], which is dioxus/web with NO
# fullstack. option_env! bakes ASSET_BASE_URL into the WASM at compile time.
ASSET_BASE_URL="$ASSET_BASE_URL" dx build --release --platform web

# dx writes the static bundle here for a web build.
BUNDLE="target/dx/my-holiday/release/web/public"
echo "    bundle: $BUNDLE"

if [ -n "${CF_PAGES_PROJECT:-}" ]; then
    echo "==> 5/5 Deploying to Cloudflare Pages ($CF_PAGES_PROJECT)"
    wrangler pages deploy "$BUNDLE" --project-name "$CF_PAGES_PROJECT"
else
    echo "==> 5/5 Skipped (set CF_PAGES_PROJECT to deploy). Bundle ready at:"
    echo "    $BUNDLE"
fi

echo "Done."
