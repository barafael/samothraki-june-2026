# Samothraki Holiday

A [Dioxus](https://dioxuslabs.com/) fullstack app that plots the photos and
videos from the trip on a map, and lets you place the ones that have no GPS.

- **Map** tab: every located photo/video; click a marker to view it, step
  through them in time order, filter by day, double-click for fullscreen.
- **Annotate** tab: pick a file without a location, click the map (or type
  lat/lng) and save. Saving needs the annotation password.

## Where things live

| What | Where |
| --- | --- |
| App | [fly.io](https://fly.io), built from `Dockerfile`, deployed on push to `main` |
| Web-sized photos (~4 MP JPEG) | GitHub Release [`photos`](https://github.com/barafael/samothraki-june-2026/releases/tag/photos) |
| Browser-playable videos (H.264) | GitHub Release [`videos`](https://github.com/barafael/samothraki-june-2026/releases/tag/videos) |
| Locations + timestamps | `assets/photo_data.json` in this repo (the deployed app commits annotations here) |
| Files still lacking a location | `assets/photos_no_gps.json` |
| Originals (~7 GB) | your disk (`Photos-3-001/`, gitignored), and Git LFS history of this repo (commit `0d846c0`) |

The browser only ever talks to the app: `/photos/<file>` and `/videos/<file>`
are served by the app's server, which fetches each file from the release once
and caches it. See [deploy.md](deploy.md) for setup and configuration.

## Develop locally

Needs Rust with the `wasm32-unknown-unknown` target and the Dioxus CLI 0.7.9
(`cargo binstall dioxus-cli@0.7.9`).

```sh
dx serve
```

Locally, photo data is read from and written to `assets/photo_data.json`, and
saving needs no password (debug builds only). Media comes from local folders
when they exist — `dist-media/{photos,videos}`, then `Photos-3-001/` for
photos — and otherwise from the GitHub releases, so a fresh clone works too.
When an annotated original is on disk, its EXIF GPS is updated as well
(`scripts/write_exif_gps.py`; uses `exiftool` when installed).

Tests: `cargo test --no-default-features --features server` and
`cargo test -p photo-extract`.

## Publish media

`scripts/publish_media.sh` makes the web-sized copies and uploads whatever is
missing from the `photos` / `videos` releases (needs `gh`, `ffmpeg`):

```sh
scripts/publish_media.sh                       # originals from ./Photos-3-001
LFS_COMMIT=0d846c0 scripts/publish_media.sh    # originals from Git LFS history
```

Or run the **Publish media to GitHub Releases** workflow (Actions tab), which
does the LFS variant on a GitHub runner. Both are incremental.

## Re-extract metadata

`cargo run -p photo-extract --bin extract` rebuilds `assets/photo_data.json`
from the originals' EXIF/ffprobe data. It starts from scratch, so manual
annotations are lost — commit first and merge them back if you ever need it.
