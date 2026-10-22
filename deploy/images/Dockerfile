FROM node:22-bookworm AS web
WORKDIR /web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web ./
ARG VITE_AGENT_URL=http://127.0.0.1:8790
ENV VITE_AGENT_URL=$VITE_AGENT_URL
RUN npm run build

FROM rust:1.99-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY lab ./lab
COPY procedures ./procedures
ENV CARGO_BUILD_JOBS=2
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p darkbox-cli -p darkbox-lab \
    && mkdir -p /out \
    && cp /src/target/release/darkbox /src/target/release/darkbox-lab /out/

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl nmap python3 python3-venv python3-pip \
    && rm -rf /var/lib/apt/lists/*
COPY adapters/browser-python/requirements.txt /tmp/browser-requirements.txt
RUN python3 -m venv /opt/browser \
    && /opt/browser/bin/pip install --no-cache-dir -r /tmp/browser-requirements.txt \
    && /opt/browser/bin/playwright install --with-deps chromium \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /out/darkbox /usr/local/bin/darkbox
COPY --from=builder /out/darkbox-lab /usr/local/bin/darkbox-lab
COPY procedures ./procedures
COPY adapters ./adapters
COPY --from=web /web/dist ./web
RUN mkdir -p /data/objects
ENV DARKBOX_REPO_ROOT=/app \
    DARKBOX_BLOB_ROOT=/data/objects \
    DARKBOX_WEB_DIST=/app/web \
    DARKBOX_ADAPTER_PYTHON=/opt/browser/bin/python \
    DARKBOX_BROWSER_MODE=auto
EXPOSE 8787 8080 9099
CMD ["darkbox", "serve", "--bind", "0.0.0.0:8787"]
