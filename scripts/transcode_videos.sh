#!/usr/bin/env bash
# Transcode HEVC/H.265 videos in Photos-3-001/ to browser-playable H.264/AAC.
#
# Pixel .mp4 / .LS.mp4 files are HEVC, which browsers can't decode in <video>.
# We write web-playable copies to media-web/ (gitignored derived artifacts),
# served at /media/<file> by the axum router. Originals are never modified.
#
# Idempotent: skips a target that already exists and is newer than its source.
set -euo pipefail

SRC_DIR="Photos-3-001"
OUT_DIR="media-web"
mkdir -p "$OUT_DIR"

shopt -s nullglob
count=0
skipped=0
for src in "$SRC_DIR"/*.mp4 "$SRC_DIR"/*.mov; do
    [ -e "$src" ] || continue
    name="$(basename "$src")"
    out="$OUT_DIR/${name%.*}.mp4"

    if [ -f "$out" ] && [ "$out" -nt "$src" ]; then
        skipped=$((skipped + 1))
        continue
    fi

    echo "  transcoding $name"
    # H.264 + AAC; faststart moves the moov atom up for streaming. Map the first
    # video and (optionally) first audio stream, dropping the motion-photo
    # metadata streams. The "-map 0:a:0?" token is quoted so the shell/ffmpeg
    # don't treat the trailing "?" specially.
    ffmpeg -y -loglevel error -i "$src" \
        -map "0:v:0" -map "0:a:0?" \
        -c:v libx264 -preset veryfast -crf 23 -pix_fmt yuv420p \
        -c:a aac -b:a 128k \
        -movflags +faststart \
        "$out"
    count=$((count + 1))
done

echo "Transcoded $count video(s); skipped $skipped up-to-date."
