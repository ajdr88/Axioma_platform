# Scope-downs pass (FR-CORE-14, OCR) — the first-ever Dockerfile for apps/api. `apps/api` runs
# on the host (`cargo run -p api`) for normal dev; this image exists specifically so OCR
# (`--features ocr`) can be built and run against real Tesseract/Pdfium without requiring every
# contributor's machine — this one included, Windows — to have those native libraries installed
# just to build the workspace. Build from the REPO ROOT (this crate has sibling `path` deps on
# packages/alf-lite, packages/cem-core, packages/sysml-core, packages/sysml-textual):
#   docker compose build api
# Not optimized for layer-caching (a plain `COPY . .`, no dependency-only pre-build layer) — a
# real, accepted simplicity tradeoff for a first version, not an oversight.

FROM rust:1-slim-bookworm AS build
# `protobuf-compiler` (real Debian bookworm package, verified: 3.21.12-3+deb12u1) — apps/api's
# own build.rs compiles fuml_runtime.proto/cem_archspace.proto via tonic-build/prost-build, which
# needs a `protoc` binary and doesn't vendor one. build.rs prefers a local `tools/protoc.exe` if
# present (the host's own Windows dev copy) over this one — excluded via .dockerignore for
# exactly that reason (a real bug found via a live build: copying that Windows .exe into this
# Linux image made build.rs try to execute it here, which fails, not falls back cleanly).
RUN apt-get update && apt-get install -y --no-install-recommends \
    libtesseract-dev libleptonica-dev clang pkg-config curl ca-certificates protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# A real, pinned bblanchon/pdfium-binaries release (verified directly: `lib/libpdfium.so` is its
# real internal path) — pdfium-render dynamically binds this at runtime, never bundles it.
RUN curl -sL -o /tmp/pdfium.tgz \
    https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8035/pdfium-linux-x64.tgz \
    && mkdir -p /opt/pdfium \
    && tar -xzf /tmp/pdfium.tgz -C /opt/pdfium lib/libpdfium.so --strip-components=1 \
    && rm /tmp/pdfium.tgz

WORKDIR /build
COPY . .
RUN cargo build --release --features ocr -p api

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    tesseract-ocr tesseract-ocr-eng ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /build/target/release/api ./api
COPY --from=build /opt/pdfium/libpdfium.so ./libpdfium.so

ENV PDFIUM_LIB_PATH=/app/libpdfium.so
EXPOSE 8080
CMD ["./api"]
