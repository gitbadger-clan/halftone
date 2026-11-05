# syntax=docker/dockerfile:1
# check=skip=InvalidDefaultArgInFrom
# (UV_VERSION deliberately has no default: references.py always passes the pin, and
# a default would be an unpinned version for anyone building by hand.)
# The differential run's reference tools, each built from a pinned source:
# ExifTool, OpenSSL, c2patool, Credentio's c2pa_validate and a conformance-public
# checkout (the C2PA rubrics), plus uv and Python for the collector. Halftone is not
# in the image: ht and `cargo test` stay on the host, only collection runs here.
#
# The pins live in docker/references.env. Build and run through
# scripts/references.py, which passes every pin as a build argument and tags the
# image from the pins and this file, so a pin change is a new tag.
#
# What the pins fix is every version a header records. Not pinned, so a rebuild of
# the same tag is not bit-for-bit identical: the base images (debian:bookworm[-slim],
# rust:1-bookworm), apt packages, the LLVM point release llvm.sh installs, and the
# Python 3.14 patch release uv installs.
#
# The build context is empty (references.dockerfile.dockerignore ignores
# everything): nothing from the checkout goes into the image. The corpus is
# bind-mounted at run time.

ARG UV_VERSION
FROM ghcr.io/astral-sh/uv:${UV_VERSION} AS uv

# --- OpenSSL: the collector reads signing certificates with `openssl x509`, and the
# version goes into every header, so it is built from the release tarball rather
# than taken from Debian (3.0.x). c2patool links against it too.
FROM debian:bookworm-slim AS openssl
ARG OPENSSL_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential perl curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN test -n "$OPENSSL_VERSION" \
    && curl -fsSL -o /tmp/openssl.tar.gz \
        "https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz" \
    && mkdir /tmp/openssl && tar -xzf /tmp/openssl.tar.gz -C /tmp/openssl --strip-components=1 \
    && cd /tmp/openssl \
    && ./Configure --prefix=/opt/openssl --openssldir=/opt/openssl/ssl --libdir=lib \
        no-tests no-docs '-Wl,-rpath,/opt/openssl/lib' \
    && make -j"$(nproc)" && make install_sw install_ssldirs \
    && rm -rf /tmp/openssl /tmp/openssl.tar.gz

# --- ExifTool: the GitHub tag archive of the release, not Debian's package.
FROM debian:bookworm-slim AS exiftool
ARG EXIFTOOL_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN test -n "$EXIFTOOL_VERSION" \
    && mkdir /opt/exiftool \
    && curl -fsSL "https://github.com/exiftool/exiftool/archive/refs/tags/${EXIFTOOL_VERSION}.tar.gz" \
        | tar -xz -C /opt/exiftool --strip-components=1 \
    && rm -rf /opt/exiftool/t /opt/exiftool/html

# --- c2patool: the crate at its pinned version with its own lock file. It moves
# with Halftone's c2pa (0.28.x is c2pa 0.91), see scripts/references.py bump.
FROM rust:1-bookworm AS c2patool
ARG C2PATOOL_VERSION
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config cmake clang \
    && rm -rf /var/lib/apt/lists/*
COPY --from=openssl /opt/openssl /opt/openssl
ENV OPENSSL_DIR=/opt/openssl
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    test -n "$C2PATOOL_VERSION" \
    && RUSTFLAGS="-C link-arg=-Wl,-rpath,/opt/openssl/lib" \
       cargo install c2patool --version "$C2PATOOL_VERSION" --locked --root /opt/c2patool

# --- Credentio: c2pa_validate at a pinned commit, built the way its README and
# Ubuntu CI do (Clang with libc++ from apt.llvm.org, Bazelisk, which downloads the
# Bazel version in the repo's .bazelversion). Bazel runs unprivileged: some rules
# refuse to run as root. Its cache is a BuildKit cache mount, so a commit bump
# rebuilds incrementally.
FROM debian:bookworm AS credentio
ARG CREDENTIO_COMMIT
ARG BAZELISK_VERSION
ARG LLVM_VERSION
ARG BAZEL_JOBS=4
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
        lsb-release wget curl ca-certificates gnupg software-properties-common \
        git python3 build-essential zip unzip \
    && wget -qO /tmp/llvm.sh https://apt.llvm.org/llvm.sh && chmod +x /tmp/llvm.sh \
    && /tmp/llvm.sh "$LLVM_VERSION" all \
    && update-alternatives --install /usr/bin/clang clang "/usr/bin/clang-${LLVM_VERSION}" 100 \
    && update-alternatives --install /usr/bin/clang++ clang++ "/usr/bin/clang++-${LLVM_VERSION}" 100 \
    && rm -rf /var/lib/apt/lists/* /tmp/llvm.sh
RUN test -n "$BAZELISK_VERSION" \
    && curl -fsSL -o /usr/local/bin/bazel \
        "https://github.com/bazelbuild/bazelisk/releases/download/${BAZELISK_VERSION}/bazelisk-linux-${TARGETARCH}" \
    && chmod +x /usr/local/bin/bazel
RUN useradd -m -u 1000 builder
USER builder
WORKDIR /home/builder
RUN test -n "$CREDENTIO_COMMIT" \
    && git clone --no-checkout https://mediaprovenance.googlesource.com/credentio src \
    && git -C src checkout --detach "$CREDENTIO_COMMIT"
# The binary is named c2pa_validate-<commit>, the form the collector reads the
# version from (differential.py credentio_version). The libc++ libraries it links
# are copied next to it for the runtime stage.
# The cache mount is all of ~/.cache, where Bazelisk keeps the Bazel it downloads
# and Bazel its output base. Mounting only ~/.cache/bazel would leave BuildKit to
# create ~/.cache owned by root, and Bazelisk could not write ~/.cache/bazelisk.
RUN --mount=type=cache,target=/home/builder/.cache,uid=1000,gid=1000,id=credentio-cache \
    cd src \
    && bazel build --jobs="$BAZEL_JOBS" tools:c2pa_validate \
    && short="$(git rev-parse --short=7 HEAD)" \
    && mkdir -p /home/builder/out/lib \
    && cp -L bazel-bin/tools/c2pa_validate "/home/builder/out/c2pa_validate-${short}" \
    && ldd "/home/builder/out/c2pa_validate-${short}" \
        | awk '/libc\+\+|libunwind/ { print $3 }' \
        | xargs -r -I{} cp -L {} /home/builder/out/lib/ \
    && bazel shutdown

# --- conformance-public: the C2PA rubrics and their evaluators, kept as a git
# checkout because the collector records its commit.
FROM debian:bookworm-slim AS conformance
ARG CONFORMANCE_COMMIT
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN test -n "$CONFORMANCE_COMMIT" \
    && git clone --no-checkout https://github.com/c2pa-org/conformance-public /opt/conformance-public \
    && git -C /opt/conformance-public checkout --detach "$CONFORMANCE_COMMIT"

# --- runtime
FROM debian:bookworm-slim
ARG UV_VERSION
ARG EXIFTOOL_VERSION
ARG OPENSSL_VERSION
ARG C2PATOOL_VERSION
ARG CREDENTIO_COMMIT
ARG CONFORMANCE_COMMIT
ARG PINS_TAG=unknown
LABEL org.halftone.references.pins=$PINS_TAG \
      org.halftone.references.uv=$UV_VERSION \
      org.halftone.references.exiftool=$EXIFTOOL_VERSION \
      org.halftone.references.openssl=$OPENSSL_VERSION \
      org.halftone.references.c2patool=$C2PATOOL_VERSION \
      org.halftone.references.credentio=$CREDENTIO_COMMIT \
      org.halftone.references.conformance=$CONFORMANCE_COMMIT
RUN apt-get update && apt-get install -y --no-install-recommends git perl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=uv /uv /uvx /usr/local/bin/
COPY --from=openssl /opt/openssl /opt/openssl
COPY --from=exiftool /opt/exiftool /opt/exiftool
COPY --from=c2patool /opt/c2patool /opt/c2patool
COPY --from=credentio /home/builder/out/ /opt/credentio/
COPY --from=conformance /opt/conformance-public /opt/conformance-public
# Python for the scripts, installed once at build time; uv never downloads one at
# run time. The scripts' packages are synced into the uv cache volume on first use.
ENV UV_PYTHON_INSTALL_DIR=/opt/uv-python
RUN uv python install 3.14
# safe.directory: the repo and the rubrics checkout belong to another user in the
# container. /cache is world-writable because a named volume starts as a copy of
# it, so whatever --user runs the container can write the uv cache.
RUN if [ -n "$(ls -A /opt/credentio/lib)" ]; then cp /opt/credentio/lib/* /usr/local/lib/; fi \
    && rm -r /opt/credentio/lib && ldconfig \
    && cd /opt/credentio && ln -s c2pa_validate-* c2pa_validate \
    && ln -s /opt/exiftool/exiftool /usr/local/bin/exiftool \
    && git config --system --add safe.directory '*' \
    && mkdir -p /cache/uv && chmod 1777 /cache /cache/uv
ENV PATH=/opt/openssl/bin:/opt/c2patool/bin:$PATH \
    CREDENTIO=/opt/credentio/c2pa_validate \
    RUBRICS=/opt/conformance-public \
    UV_CACHE_DIR=/cache/uv \
    UV_PYTHON_DOWNLOADS=never \
    UV_LOCKED=1 \
    HOME=/tmp
# Fail the build, not the first collection, if a tool cannot start.
RUN exiftool -ver && c2patool --version && openssl version \
    && ! ldd /opt/credentio/c2pa_validate | grep 'not found' \
    && ! ldd /opt/c2patool/bin/c2patool | grep 'not found' \
    && test -f /opt/conformance-public/asset-rubrics/c2pa_signals_rubric_evaluator.py
WORKDIR /work
