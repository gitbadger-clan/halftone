# syntax=docker/dockerfile:1
# Linux image for offline checks: ht (this checkout) plus the two reference
# c2patools (c2pa 0.90 and 0.91). Run with --network none; see scripts/offline-docker.fish.
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev cmake clang \
    && rm -rf /var/lib/apt/lists/*
# Reference tools first, in their own layers, so a Halftone change doesn't rebuild them.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install c2patool --version 0.27.22 --locked --root /opt/c2patool-0.27
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install c2patool --version 0.28.0 --locked --root /opt/c2patool-0.28
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=halftone-target \
    cargo build --release --locked -p halftone-cli --features c2pa \
    && cp target/release/ht /usr/local/bin/ht

FROM debian:bookworm-slim
# Which checkout this ht was built from; offline-docker.fish rebuilds when it differs.
ARG HALFTONE_COMMIT=unknown
LABEL org.halftone.commit=$HALFTONE_COMMIT
RUN apt-get update && apt-get install -y --no-install-recommends python3 strace ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /opt/c2patool-0.27 /opt/c2patool-0.27
COPY --from=build /opt/c2patool-0.28 /opt/c2patool-0.28
COPY --from=build /usr/local/bin/ht /usr/local/bin/ht
