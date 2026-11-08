# syntax=docker/dockerfile:1

# Multi-stage build for the six xdu binaries (xdu, xdu-find, xdu-view, xdu-rm, xdu-cp, xdu-mv)
# and the DuckDB extensions they load from /usr/local/lib/xdu/extensions.
#
# Usage:
#   docker build -t xdu .
#   docker run --rm xdu xdu --help
#
# Copy binaries into another image, with the extensions beside them:
#   COPY --from=ghcr.io/xdu-project/xdu:latest /usr/local/bin/xdu* /usr/local/bin/
#   COPY --from=ghcr.io/xdu-project/xdu:latest /usr/local/lib/xdu/ /usr/local/lib/xdu/
#
# Base images are tag-pinned to Debian bookworm (builder/runtime glibc + libstdc++ match).
# For full supply-chain reproducibility, DIGEST-PIN both before the first container release:
#   docker buildx imagetools inspect rust:1-slim-bookworm    --format '{{.Manifest.Digest}}'
#   docker buildx imagetools inspect debian:bookworm-slim    --format '{{.Manifest.Digest}}'
# then append `@sha256:<digest>` to each FROM line.

# =============================================================================
# Build stage
# =============================================================================
FROM rust:1-slim-bookworm AS builder

# The duckdb crate's `bundled` feature compiles DuckDB from C++ source; cc-rs invokes a tool
# literally named `c++`, which rust:1-slim-bookworm does not ship (it has cc/gcc only). curl
# fetches the pinned DuckDB extensions below.
RUN apt-get update && \
    apt-get install -y --no-install-recommends g++ curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Pin the toolchain first: rust-toolchain.toml is the single source of truth, so it must be
# present before any cargo invocation selects/downloads the pinned toolchain.
COPY rust-toolchain.toml ./
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
COPY packaging/extensions ./packaging/extensions

# BuildKit cache mounts keep the cargo registry and the (heavy, bundled-DuckDB-C++) target
# directory warm across builds, so DuckDB is not recompiled from scratch every time. A cache
# mount is NOT persisted in the image layer, so the freshly built binaries are copied OUT of
# the cache-mounted target dir to /out within the same RUN, before this stage ends.
# --locked pins dependency resolution to the committed Cargo.lock. The extension probe comes
# from the same cargo build as the binaries, so it reports the engine they carry; it is copied
# out beside /out, never into it, so it does not ship.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
        --bin xdu --bin xdu-find --bin xdu-view --bin xdu-rm --bin xdu-cp --bin xdu-mv \
        --example extension-probe && \
    mkdir -p /out/bin /probe && \
    cp target/release/xdu \
       target/release/xdu-find \
       target/release/xdu-view \
       target/release/xdu-rm \
       target/release/xdu-cp \
       target/release/xdu-mv \
       /out/bin/ && \
    cp target/release/examples/extension-probe /probe/

# Fetch the pinned extensions for the engine just built into the staged tree, then prove that
# tree loads them under a cold HOME, as a release leg does with its tarball. A mismatched or
# unloadable extension fails the image build here.
RUN sh packaging/extensions/fetch.sh /probe/extension-probe /out/lib/xdu/extensions && \
    tar czf /tmp/xdu-image.tar.gz -C /out bin lib && \
    sh packaging/extensions/check.sh /tmp/xdu-image.tar.gz /probe/extension-probe && \
    rm -f /tmp/xdu-image.tar.gz

# =============================================================================
# Runtime stage
# =============================================================================
FROM debian:bookworm-slim AS runtime

# The bundled DuckDB (C++) dynamically links libstdc++/libgcc; ca-certificates is general
# hygiene. bookworm-slim already ships libstdc++6 and libgcc-s1, so this RUN
# effectively installs only ca-certificates; libstdc++6 stays listed to pin the
# runtime contract the DuckDB-linked binaries need against a future slim that drops it.
RUN apt-get update && \
    apt-get install -y --no-install-recommends libstdc++6 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Run as a non-root user rather than root.
RUN groupadd --system xdu && \
    useradd --system --gid xdu --home-dir /home/xdu --create-home xdu

# Install the checked tree whole: the six user-facing binaries (release profile already strips
# them) and lib/xdu/extensions beside them, so the image holds exactly what check.sh loaded.
COPY --from=builder /out/ /usr/local/
RUN test -f /usr/local/lib/xdu/extensions/httpfs.duckdb_extension

USER xdu

ENTRYPOINT ["/usr/local/bin/xdu"]
