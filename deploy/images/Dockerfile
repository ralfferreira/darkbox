FROM node:22-bookworm AS web
WORKDIR /web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci
COPY apps/web ./
RUN npm run build

FROM rust:1.99-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY lab ./lab
COPY procedures ./procedures
RUN cargo build --release -p darkbox-cli -p darkbox-lab

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3 curl nmap \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/darkbox /usr/local/bin/darkbox
COPY --from=builder /src/target/release/darkbox-lab /usr/local/bin/darkbox-lab
COPY procedures ./procedures
COPY adapters ./adapters
COPY --from=web /web/dist ./web
RUN mkdir -p /data/objects
ENV DARKBOX_REPO_ROOT=/app \
    DARKBOX_BLOB_ROOT=/data/objects \
    DARKBOX_WEB_DIST=/app/web
EXPOSE 8787 8080 9099
CMD ["darkbox", "serve", "--bind", "0.0.0.0:8787"]
