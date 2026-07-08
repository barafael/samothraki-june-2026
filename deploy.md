# Deploy

The app and its media deploy **independently**:

- **App** (~2 MB: HTML/WASM/JS/CSS) → **GitHub Pages**, via
  `.github/workflows/deploy.yml` on every push to `main`.
- **Media** (~10 GB: photos, thumbnails, videos, `manifest.json`) →
  **Cloudflare R2**, synced from your machine with `scripts/publish.sh`.

Media can't live on Pages: GitHub blocks files over **100 MiB** (some videos are
larger), caps published sites at **1 GB** (the photos are ~6.8 GB), and Pages has
a **100 GB/month** bandwidth soft limit. R2 has no per-file cap and **zero egress
fees**, so it hosts the media.

## The two URLs (don't mix them up)

Replace `<user>` with your GitHub username (e.g. the owner of this repo). The
repository is `samothraki-june-2026`.

| Thing | Value | Note |
| --- | --- | --- |
| **Site URL** (where the app loads) | `https://<user>.github.io/samothraki-june-2026/` | Project site — **includes** the repo name as a path. |
| **Pages origin** (for CORS) | `https://<user>.github.io` | Origin = scheme + host only, **no path**. |

Why they differ: a browser's CORS check matches on **origin** (scheme + host),
never on path. Putting the `/samothraki-june-2026/` path in the R2 `AllowedOrigins`
would never match and would silently break the `manifest.json` fetch. Use the
bare origin for CORS, the full path for the site link.

The workflow passes `--base-path samothraki-june-2026` (from
`github.event.repository.name`) so the app's own assets resolve under that
subpath. `ASSET_BASE_URL` (R2) is absolute, so media is unaffected by the base
path.

## One-time Cloudflare (R2) setup

1. Create an R2 bucket, e.g. `samothraki-holiday`.
2. Enable public access — the auto-generated `https://pub-<hash>.r2.dev` URL is
   fine (or attach a custom domain for CDN caching + a stable URL). This URL,
   with no trailing slash, is your `ASSET_BASE_URL`.
3. **Add a CORS policy** on the bucket. The viewer `fetch`es `manifest.json`
   cross-origin, so without this the browser blocks it (plain `<img>`/`<video>`
   don't need CORS, but the manifest fetch does). Use the bare **origin**:

   ```json
   [{ "AllowedOrigins": ["https://<user>.github.io"],
      "AllowedMethods": ["GET"],
      "AllowedHeaders": ["*"] }]
   ```

4. Create an R2 API token (Object Read/Write) and configure it as an `rclone`
   remote named to match `R2_BUCKET` in `scripts/publish.sh`
   (e.g. `r2:samothraki-holiday`).

## One-time GitHub setup

1. **Settings → Pages → Build and deployment → Source = GitHub Actions.**
2. **Settings → Secrets and variables → Actions → Variables** → add
   `ASSET_BASE_URL` = your R2 public URL, e.g. `https://pub-<hash>.r2.dev`
   (no trailing slash). The workflow bakes this into the WASM; if it's unset the
   build fails loudly rather than shipping a viewer that points at Pages.

## First publish (initial full upload)

CI can't upload media — the 10 GB of originals aren't in git. Do the first
upload from your machine:

```sh
export ASSET_BASE_URL="https://pub-<hash>.r2.dev"   # same value as the repo variable
export R2_BUCKET="r2:samothraki-holiday"            # your rclone remote:bucket
scripts/publish.sh
```

`publish.sh` transcodes videos → generates `manifest.json` + thumbnails →
`rclone sync`s photos, videos, thumbs, and the manifest to R2 → builds the
viewer. (The build step it runs is for local verification; Pages is deployed by
CI, below.)

## Ongoing updates

- **Changed the app?** Push to `main`. Actions rebuilds and redeploys Pages.
  Watch it under the repo's **Actions** tab; the deployed URL appears on the
  `deploy` job.
- **Added or annotated photos?** Run `scripts/publish.sh` again to push the new
  media and `manifest.json` to R2. No redeploy needed — the app fetches the
  manifest at runtime.

## How ASSET_BASE_URL is baked in (gotcha)

`dx build` does **not** surface a shell env var to `option_env!` in the final
wasm. `build.rs` works around this: it reads `ASSET_BASE_URL` and writes an
`asset_base_url.rs` into `OUT_DIR` that `src/config.rs` `include!`s. That's
compile-input, so it survives dx under both `cargo` and CI. Don't remove
`build.rs`, and don't "simplify" `config.rs` back to a plain `option_env!`.

Verify a build baked the right URL:

```sh
strings target/wasm32-unknown-unknown/wasm-release/my-holiday.wasm | grep r2.dev
```

## Local preview without R2

To preview the static viewer locally (root-relative assets, no R2):

```sh
scripts/setup.sh
python3 -m http.server -d target/dx/my-holiday/release/web/public 8080
```
