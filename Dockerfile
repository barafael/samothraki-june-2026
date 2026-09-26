# syntax=docker/dockerfile:1
#
# The fullstack app for fly.io: one `server` binary that serves the page, the
# WASM client in public/, the photo-data server functions, and /photos +
# /videos (proxied from this repo's GitHub Releases). No media is baked in.

FROM rust:1-trixie AS builder
ARG DX_VERSION=0.7.9
RUN rustup target add wasm32-unknown-unknown \
 && curl -fsSL "https://github.com/DioxusLabs/dioxus/releases/download/v${DX_VERSION}/dx-x86_64-unknown-linux-gnu.tar.gz" \
    | tar xz -C /usr/local/bin \
 && dx --version
WORKDIR /app
COPY . .
# Cache mounts keep crates, build artifacts, and dx's downloaded tools
# (wasm-bindgen, wasm-opt, esbuild) warm between deploys.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/root/.local/share/.dx \
    dx bundle --platform web --release \
 && cp -r target/dx/my-holiday/release/web /out

FROM debian:trixie-slim
# CA roots for HTTPS to GitHub (the builder image already has them).
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
WORKDIR /app
COPY --from=builder /out/ ./
# Fallback photo data, used when PHOTO_DATA_GITHUB_TOKEN is not set (then the
# deployed app is read-only unless ANNOTATE_PASSWORD is set, and saves only
# last until the machine restarts).
COPY assets/photo_data.json assets/photo_data.json
ENV IP=0.0.0.0 \
    PORT=8080 \
    MEDIA_CACHE_DIR=/tmp/media-cache
EXPOSE 8080
CMD ["./server"]
