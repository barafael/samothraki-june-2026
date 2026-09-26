# Deploy

Three pieces, each hosted where it fits:

- **App** → **fly.io**. One Rust binary (`server`) built from `Dockerfile`:
  serves the page and WASM client, the photo-data server functions, and
  `/photos/<file>` + `/videos/<file>`. Redeployed by
  `.github/workflows/fly-deploy.yml` on every push to `main`.
- **Media** → **GitHub Releases** of this repo, tags `photos` (~4 MP JPEGs,
  same filenames as the originals) and `videos` (H.264 `<stem>.mp4`). The app
  downloads each file from there on first request and caches it on the
  machine. Releases have no bandwidth quota and a 2 GiB per-file limit, so they
  suit the ~1.5 GB of web-sized media better than Git LFS (metered bandwidth) or
  plain git (100 MiB per file).
- **Photo data** → `assets/photo_data.json` in this repo. With
  `PHOTO_DATA_GITHUB_TOKEN` set, the app reads it from GitHub and commits each
  annotation there.

## 1. Publish the media (once, then whenever photos are added)

Either on your machine, from the originals:

```sh
gh auth login                    # once
scripts/publish_media.sh         # reads ./Photos-3-001, needs ffmpeg
```

or on GitHub: **Actions → Publish media to GitHub Releases → Run workflow**.
That takes the originals from this repo's Git LFS history (commit `0d846c0`,
the last one with `Photos-3-001/` in LFS). It downloads every original it still
needs to process — ~7.3 GB the first time — and that counts against the repo
owner's Git LFS bandwidth quota, so prefer the local run if you still have the
files. Both variants are incremental and resume after an interruption.

Check: https://github.com/barafael/samothraki-june-2026/releases should list
`photos` (666 assets) and `videos` (104 assets).

## 2. Create the fly.io app (once)

```sh
fly auth login
fly apps create samothraki-june-2026     # or pick a free name and put it in fly.toml
```

Secrets (encrypted, not in `fly.toml`):

```sh
# Password for the Annotate tab. Without it the deployed app is read-only.
fly secrets set ANNOTATE_PASSWORD='something-long'

# Lets the app commit annotations to assets/photo_data.json. Create a
# fine-grained token at https://github.com/settings/personal-access-tokens:
# repository access "Only select repositories" -> samothraki-june-2026,
# permission Contents: Read and write.
fly secrets set PHOTO_DATA_GITHUB_TOKEN='github_pat_...'
```

First deploy by hand (or skip to step 3 and push):

```sh
fly deploy
```

The app is then at `https://<app>.fly.dev`.

## 3. Deploy from GitHub Actions (once)

```sh
fly tokens create deploy -x 999999h
```

Add the output as the repository secret **FLY_API_TOKEN** (Settings → Secrets
and variables → Actions). From then on every push to `main` deploys; run the
**Deploy to fly.io** workflow manually to redeploy without a push.

Pushes that only touch `assets/photo_data.json` (i.e. the app's own annotation
commits) or Markdown don't redeploy: the running app reads the data from
GitHub, so it doesn't need to.

## How annotation saving works

1. The browser calls the `save_annotation` server function with the password.
2. The server checks it against `ANNOTATE_PASSWORD`, re-reads
   `photo_data.json` from GitHub (conditional request, so usually a cheap
   304), updates the entry, and commits the file via the contents API
   ("Annotate <file> at <lat>, <lng>"). If the file changed on GitHub in the
   meantime it re-reads and retries once.
3. Page loads always get the latest committed data, including edits pushed
   from your machine.

Pull before editing `assets/photo_data.json` locally — the app may have
committed to it.

Without `PHOTO_DATA_GITHUB_TOKEN` the app serves the copy of
`photo_data.json` baked into the image; annotations saved there (only possible
with `ANNOTATE_PASSWORD` set) are lost when the machine restarts.

## Configuration

Environment variables read by the server:

| Variable | Default | Purpose |
| --- | --- | --- |
| `ANNOTATE_PASSWORD` | unset | Required to save annotations. Unset: saves refused in release builds, allowed in local debug builds. |
| `PHOTO_DATA_GITHUB_TOKEN` | unset | Enables reading/committing `assets/photo_data.json` on GitHub. |
| `GITHUB_REPO` | `barafael/samothraki-june-2026` | Repo with the media releases and the photo data. |
| `GITHUB_BRANCH` | `main` | Branch the photo data is read from and committed to. |
| `PHOTO_DATA_PATH` | `assets/photo_data.json` | Local photo data (development, and fallback). |
| `MEDIA_DIR` | `dist-media` | Local `photos/` + `videos/` served before GitHub is asked. |
| `MEDIA_CACHE_DIR` | system temp dir | Where downloaded release assets are cached. |
| `MEDIA_BASE_URL` | `https://github.com/$GITHUB_REPO/releases/download` | Where `/<photos\|videos>/<file>` is fetched from. |
| `IP`, `PORT` | `127.0.0.1`, `8080` | Listen address (the image sets `0.0.0.0:8080`). |

## Costs and limits

- fly.io: one `shared-cpu-1x` / 512 MB machine that stops when idle
  (`auto_stop_machines`), plus outbound traffic for the photos viewers load.
- The machine's disk is ephemeral: after a restart or deploy, media is fetched
  from GitHub again on demand (about a second per photo, once).
- GitHub Releases: at most 1000 assets per release — 666 photos and 104 videos
  fit. Add a second release (and a lookup for it) if the photo count ever grows
  past that.
