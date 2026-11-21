# Dev-container image for HarvestLock Contracts (#300).
#
# Pins the Rust toolchain CI runs on and installs everything `cargo test` and
# `stellar contract build` need, so a fresh container can run both with no
# extra steps. Two choices worth knowing:
#
# - `stellar` comes from the official prebuilt release, not `cargo install`:
#   the from-source build needs MinGW binutils on Windows and libdbus/libudev
#   dev headers on Linux. Those are the two gotchas in
#   docs/dev-setup-troubleshooting.md that this image exists to avoid; the
#   dev headers are still installed so the binary always finds its runtime
#   libs, and so a later `cargo install` still works if someone wants one.
# - Rust is pinned by tag (1.100.0) rather than `latest`, matching the "pinned
#   toolchain" requirement rather than silently drifting from CI.
FROM rust:1.100.0-slim-bookworm

ARG STELLAR_CLI_VERSION=30.0.1
ARG JUST_VERSION=1.42.4
# BuildKit sets TARGETARCH (amd64/arm64); the release asset names use the
# Rust target-triple spelling instead, so map between them below.
ARG TARGETARCH=amd64
ARG NODE_MAJOR=22

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
        pkg-config \
        libdbus-1-dev \
        libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# `cargo fmt --check` and clippy are both run locally/CI; the wasm target is
# what `stellar contract build` compiles to (not wasm32-unknown-unknown).
RUN rustup component add rustfmt clippy \
    && rustup target add wasm32v1-none

# Prebuilt CLIs: stellar-cli (the tarball contains a single `stellar`
# executable) and `just`, which mirrors the repo's CI jobs locally (#302).
RUN case "${TARGETARCH}" in \
        amd64) ARCH=x86_64 ;; \
        arm64) ARCH=aarch64 ;; \
        *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/stellar-cli.tar.gz \
        "https://github.com/stellar/stellar-cli/releases/download/v${STELLAR_CLI_VERSION}/stellar-cli-${STELLAR_CLI_VERSION}-${ARCH}-unknown-linux-gnu.tar.gz" \
    && tar -xzf /tmp/stellar-cli.tar.gz -C /usr/local/bin \
    && rm /tmp/stellar-cli.tar.gz \
    && curl -fsSL -o /tmp/just.tar.gz \
        "https://github.com/casey/just/releases/download/${JUST_VERSION}/just-${JUST_VERSION}-${ARCH}-unknown-linux-musl.tar.gz" \
    && tar -xzf /tmp/just.tar.gz -C /usr/local/bin just \
    && rm /tmp/just.tar.gz \
    && stellar --version \
    && just --version

# Node is for the TypeScript bindings and examples that live alongside the
# contract (not needed by `cargo test` itself). NodeSource's signed apt
# repository is added directly -- key downloaded and dearmored into the
# keyring apt verifies against -- rather than piping their setup script into
# a shell, so no remote content is executed while building the image.
RUN apt-get update && apt-get install -y --no-install-recommends gnupg \
    && curl -fsSL -o /tmp/nodesource.gpg.key \
        "https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key" \
    && gpg --dearmor -o /usr/share/keyrings/nodesource.gpg /tmp/nodesource.gpg.key \
    && rm /tmp/nodesource.gpg.key \
    && echo "deb [signed-by=/usr/share/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" \
        > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspaces/HarvestLock-Contracts
