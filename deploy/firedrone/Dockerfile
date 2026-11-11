# FireDrone image: flight-review-server with S3 support plus the built web app.
# Build from the repo root:
#   docker build -f deploy/firedrone/Dockerfile -t flight-review .
# The upstream Dockerfile (repo root) is left as it is, so merges stay easy.

# --- web app (SvelteKit, static) --------------------------------------------
FROM node:22-bookworm-slim AS frontend
WORKDIR /frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ ./
# Mapbox token for the map view. It ends up in the JavaScript sent to every
# browser, so use a public token restricted to the site's URL. Empty = no map.
ARG PUBLIC_MAPBOX_TOKEN=""
ENV PUBLIC_MAPBOX_TOKEN=${PUBLIC_MAPBOX_TOKEN}
RUN npm run build

# --- server ------------------------------------------------------------------
FROM rust:1.94-bookworm AS server
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
# sqlite is a default feature; s3 adds the S3 storage backend.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p flight-review-server --features s3 \
    && cp target/release/flight-review-server /usr/local/bin/

# --- runtime -----------------------------------------------------------------
FROM debian:bookworm-slim
# sqlite3: consistent backups of the live database (sqlite3 ... ".backup ...")
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl sqlite3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=server /usr/local/bin/flight-review-server /usr/local/bin/
COPY --from=frontend /frontend/build /srv/frontend

# /data holds only the SQLite database (a named volume; Docker copies this
# ownership into it on first start). Log files go to S3.
RUN useradd --system --uid 10002 --home-dir /data flightreview \
    && mkdir -p /data && chown flightreview:flightreview /data
USER flightreview

ENV FLIGHT_REVIEW_STATIC_DIR=/srv/frontend
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["flight-review-server"]
# --storage is set by docker compose (s3://bucket/prefix).
CMD ["serve", "--db", "sqlite:///data/flight-review.db"]
