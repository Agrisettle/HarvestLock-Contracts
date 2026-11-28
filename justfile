# Local mirror of .github/workflows/test.yml (#302).
#
# Every recipe below cites the CI job it reproduces. `just ci` runs the checks
# that do not need network access; the jobs that do (audit, deny, mutation
# testing, the SDK matrix) are separate recipes and are deliberately left out
# of it.
#
# Prerequisites beyond a Rust toolchain, matching what CI installs per job:
#   build, repro          stellar-cli on PATH (the devcontainer has it), and
#                         the wasm32v1-none target (`rustup target add`)
#   coverage              cargo-llvm-cov
#   audit, deny           cargo-audit, cargo-deny
#   mutants-diff          cargo-mutants
#
# See docs/dev-setup-troubleshooting.md for the native-library gotchas around
# installing those tools from source.

set shell := ["bash", "-euo", "pipefail", "-c"]

crate := "contracts/escrow"

# Run the CI checks that do not need network access.
ci: fmt test coverage permissionless-release-review deadline-boundary address-equality oracle-fault-injection snapshot-replay window-overflow build repro error-discriminants auth-docs

# CI job "cargo test" -> "Check Rust formatting".
fmt:
    cargo fmt --check --manifest-path {{ crate }}/Cargo.toml

# CI job "cargo test" -> "cargo test". Also runs every integration suite below, since they are --test targets of the same crate.
test:
    cd {{ crate }} && cargo test --locked --verbose

# CI job "code coverage".
coverage:
    cargo llvm-cov --workspace --all-targets --locked

# CI job "#2 permissionless release review".
permissionless-release-review:
    cd {{ crate }} && cargo test --locked --test permissionless_release_review --verbose

# CI job "#111 deadline boundary tests".
deadline-boundary:
    cd {{ crate }} && cargo test --locked --test deadline_boundary --verbose

# CI job "#122 Address equality audit".
address-equality:
    cd {{ crate }} && cargo test --locked --test address_equality --verbose

# CI job "#124 Reflector oracle fault injection".
oracle-fault-injection:
    cd {{ crate }} && cargo test --locked --test oracle_fault_injection --verbose

# CI job "#125 deployment snapshot replay".
snapshot-replay:
    cd {{ crate }} && cargo test --locked --test snapshot_replay -- --nocapture

# CI job "window overflow property tests".
window-overflow:
    cd {{ crate }} && cargo test --locked --test window_overflow --verbose

# CI job "stellar contract build" -> "stellar contract build". The job's apt-get and `cargo install --locked stellar-cli` steps are setup, not checks: here the CLI is expected on PATH (prebuilt, as docs/dev-setup-troubleshooting.md recommends) and the wasm target is installed by the devcontainer.
build:
    stellar contract build --locked

# CI job "stellar contract build" -> "Verify WASM build reproducibility". Builds twice from a clean target directory and fails if the hashes differ.
repro:
    #!/usr/bin/env bash
    set -euo pipefail
    wasm=target/wasm32v1-none/release/harvestlock_escrow.wasm
    stellar contract build --locked
    hash1=$(sha256sum "$wasm" | awk '{print $1}')
    rm -rf target
    stellar contract build --locked
    hash2=$(sha256sum "$wasm" | awk '{print $1}')
    echo "Build 1 hash: $hash1"
    echo "Build 2 hash: $hash2"
    if [ "$hash1" != "$hash2" ]; then
        echo "WASM build is NOT reproducible -- hashes differ between runs" >&2
        exit 1
    fi
    echo "WASM build is reproducible (hash=$hash1)"

# CI job "error discriminants".
error-discriminants:
    bash .github/scripts/check_error_discriminants.sh

# CI job "auth doc comments".
auth-docs:
    bash .github/scripts/check_auth_docs.sh

# CI job "mutation testing (changed lines)". Mirrors the PR-only job: mutate only the Rust lines this branch changes relative to `base`. Needs network to install cargo-mutants, hence outside `ci`.
mutants-diff base="main":
    #!/usr/bin/env bash
    set -euo pipefail
    git fetch --no-tags origin "+refs/heads/{{ base }}:refs/remotes/origin/{{ base }}"
    merge_base=$(git merge-base HEAD "origin/{{ base }}")
    diff_file=$(mktemp)
    trap 'rm -f "$diff_file"' EXIT
    git diff --no-ext-diff --unified=0 "$merge_base" HEAD -- '*.rs' > "$diff_file"
    if [ ! -s "$diff_file" ]; then
        echo "No Rust changes relative to {{ base }} -- nothing to mutate."
        exit 0
    fi
    cargo mutants \
        --manifest-path {{ crate }}/Cargo.toml \
        --in-diff "$diff_file" \
        --jobs 2 \
        --timeout 120

# CI job "Differential SDK Version Testing" (matrix 28.0.0 and 29.0.1). Mirrors the job's sed + test + revert sequence. Needs network to fetch the other SDK version, hence outside `ci`.
sdk-differential sdk="28.0.0":
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{ crate }}
    trap 'git checkout -- Cargo.toml' EXIT
    sed -i 's/soroban-sdk = "[^"]*"/soroban-sdk = "{{ sdk }}"/' Cargo.toml
    sed -i 's/soroban-sdk = { version = "[^"]*"/soroban-sdk = { version = "{{ sdk }}"/' Cargo.toml
    echo "Updated Cargo.toml:"
    grep -A2 -B2 soroban-sdk Cargo.toml
    cargo test --verbose

# CI job "cargo audit". Needs the RustSec database (network).
audit:
    cargo audit

# CI job "cargo deny" -> both of its steps. Needs the crates.io index (network).
deny:
    cargo deny check advisories bans
    cargo deny check licenses
