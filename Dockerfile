# ── Stage 1: Frontend ────────────────────────────────────────────────────────
FROM node:24-slim AS frontend

RUN corepack enable

WORKDIR /build

# Install dependencies first for layer caching
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile

COPY frontend/ ./
RUN pnpm run build

# ── Stage 2: Backend ─────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS backend

# OpenSSL headers needed for lettre (native-tls); libgit2 is vendored.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies before copying source.
# A dummy main lets cargo fetch and compile all deps in a cacheable layer.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY backend/src ./src
# Force Cargo to rebuild the crate (deps are already cached above).
RUN touch src/main.rs && cargo build --release

# ── Stage 3: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        pandoc \
        libssl3 \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=backend /build/target/release/backend /usr/local/bin/backend
COPY --from=frontend /build/build                 /app/static

ENV STATIC_DIR=/app/static

# config/ and data/ are expected as bind-mounts at runtime.
# config/config.json  — application config
# data/               — git-managed book repository

EXPOSE 3000
CMD ["backend"]
