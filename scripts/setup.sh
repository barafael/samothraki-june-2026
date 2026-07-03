#!/bin/bash
# Run this after photo-extract to copy assets to Dioxus output
set -e

echo "Copying photos to Dioxus output..."
mkdir -p target/dx/my-holiday/debug/web/public/assets/photos
cp -r assets/photos/* target/dx/my-holiday/debug/web/public/assets/photos/
cp assets/photo_data.json target/dx/my-holiday/debug/web/public/assets/
cp assets/photo_tags.json target/dx/my-holiday/debug/web/public/assets/
cp assets/main.css target/dx/my-holiday/debug/web/public/assets/
cp assets/photo_no_gps.json target/dx/my-holiday/debug/web/public/assets/
echo "Done. Run 'dx serve --platform web --port 8080' to start the app (or 'dx serve --fullstack --port 8080' for persistence)."
