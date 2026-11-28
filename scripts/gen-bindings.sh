#!/usr/bin/env bash
# Generate (or drift-check) the TypeScript bindings under
# `bindings/typescript/` from the escrow contract's WASM.
#
# Issue #266 asked for this: `docs/ts-bindings-comparison.md` recommends
# adopting the generated bindings — `stellar contract bindings typescript`
# output, which embeds the contract's spec — as the authoritative encoder
# for the sibling `HarvestLock` app. The bindings are only authoritative if
# they're actually *in* the repo from the same WASM every deploy ships, and
# they stay authoritative only if drift between the committed copy and the
# current WASM is caught before a PR lands. This script is both:
#
#   scripts/gen-bindings.sh          # regenerate `bindings/typescript/`
#   scripts/gen-bindings.sh --check  # regenerate into a temp dir, diff
#                                    # against the committed copy, exit
#                                    # non-zero on drift (CI mode)
#
# ## Pinned CLI version
#
# `stellar-cli 28.1.0` is what `docs/ts-bindings-comparison.md` §1 used to
# produce the committed bindings, so it is the version this script expects
# by default. A different CLI version can produce a different (though
# usually compatible) emission; the point of the drift check is to catch
# that before it merges. Override via `STELLAR_CLI_VERSION` if a deliberate
# upgrade is being made, and update the pin in this script, the doc, and
# the Reference section of the README in the same PR.
#
# ## Why this script and not a CI step
#
# `.github/PULL_REQUEST_TEMPLATE.md` says contributors do not change
# workflows — CI is maintainer-managed, part of the protocol's safety
# story. So this ships as a script + docs; a maintainer wires the
# `--check` invocation into `.github/workflows/test.yml` in a separate
# follow-up PR. The script's two modes are independently usable: a
# contributor runs it to regenerate after touching a public contract
# signature; the maintainer's eventual CI job runs `--check` to fail PRs
# that forgot to.
#
# ## Dependencies
#
# * `stellar-cli` on PATH (installed via `cargo install stellar-cli
#   --version "$STELLAR_CLI_VERSION" --locked`), used to both build the
#   WASM and emit the bindings.
# * The pinned Rust toolchain (`rust-toolchain.toml` picks it up
#   automatically) with the `wasm32v1-none` target.
# * `diff` and `sha256sum` from any standard Unix environment.
#
# ## Output
#
# A directory `bindings/typescript/` holding the full generated package:
# `package.json`, `src/`, the embedded spec, and the sdk peer dep. The
# README in that directory is kept across regenerations by excluding it
# from the overwrite (see the rsync-style step below) — the generated
# tree's own README is overwritten on each generation, but the in-repo
# README documents what the directory is *for*, not what the CLI emitted.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/gen-bindings.sh [--check]

Without --check: regenerates bindings/typescript/ from the current WASM,
overwriting whatever is there. The committed copy becomes the new
baseline; commit the result.

With --check: regenerates into a temporary directory and `diff`s it
against the committed bindings/typescript/. Exits 0 if byte-identical, 1
if drift is detected (the expected CI invocation). No files are
modified.

Environment:
  STELLAR_CLI_VERSION   Override the pinned stellar-cli version
                        (default: 28.1.0).
USAGE
}

CHECK_MODE=0
case "${1:-}" in
  --check) CHECK_MODE=1 ;;
  -h|--help) usage; exit 0 ;;
  "") ;;
  *) echo "error: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
esac

# Resolve repo root from the script's own location, so this works
# regardless of the caller's CWD (CI, a developer's shell, a git hook).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PINNED_CLI_VERSION="${STELLAR_CLI_VERSION:-28.1.0}"
WASM_PATH="target/wasm32v1-none/release/harvestlock_escrow.wasm"
COMMITTED_DIR="bindings/typescript"

if ! command -v stellar >/dev/null 2>&1; then
  cat >&2 <<EOF
error: 'stellar' not found on PATH.

Install the pinned CLI with:
  cargo install stellar-cli --version $PINNED_CLI_VERSION --locked
EOF
  exit 1
fi

# A version mismatch is a warning, not a hard error, because
# \`stellar-cli\` versions are forward-compatible for this generation
# surface in practice — but any mismatch should produce a visible note so
# an unexpected bindings-diff doesn't look like contract drift when it's
# really a CLI drift.
CLI_VERSION_OUT="$(stellar --version 2>/dev/null | head -n1 || true)"
if [[ "$CLI_VERSION_OUT" != *"$PINNED_CLI_VERSION"* ]]; then
  echo "warning: stellar-cli reports '$CLI_VERSION_OUT'; this script pins $PINNED_CLI_VERSION." >&2
  echo "         Override with STELLAR_CLI_VERSION=... if the mismatch is deliberate." >&2
fi

echo "==> building WASM at $WASM_PATH"
stellar contract build --locked

if [[ ! -f "$WASM_PATH" ]]; then
  echo "error: WASM not produced at $WASM_PATH after 'stellar contract build'." >&2
  exit 1
fi

WASM_HASH="$(sha256sum "$WASM_PATH" | awk '{print $1}')"
echo "==> WASM sha256: $WASM_HASH"

# Always emit into a fresh temp dir first, regardless of mode — so an
# interrupted run can never leave the committed tree in a half-generated
# state.
TMP_OUT="$(mktemp -d)"
trap 'rm -rf "$TMP_OUT"' EXIT

echo "==> generating bindings into $TMP_OUT"
stellar contract bindings typescript \
  --wasm "$WASM_PATH" \
  --output-dir "$TMP_OUT" \
  --overwrite

# Pin the generating WASM's hash alongside the bindings themselves, so a
# reader can tell at a glance which WASM the emitted spec came from
# without having to re-run the build. This file is diffed by `--check`
# just like the rest of the tree, so a bindings regenerated from a
# different WASM is caught by the same mechanism that catches signature
# drift.
echo "$WASM_HASH" > "$TMP_OUT/.wasm-sha256"

if [[ $CHECK_MODE -eq 1 ]]; then
  echo "==> drift check: $COMMITTED_DIR vs freshly generated"
  if [[ ! -d "$COMMITTED_DIR" ]]; then
    echo "error: $COMMITTED_DIR not present — run scripts/gen-bindings.sh (no args) to produce it." >&2
    exit 1
  fi
  # Exclude per-install artifacts that aren't part of the generated
  # output (node_modules, build artifacts). These are the same paths the
  # committed-directory .gitignore excludes — kept in sync here so the
  # diff compares the generated inputs, not what was installed/built on
  # top.
  if diff -r \
      --exclude=node_modules \
      --exclude=dist \
      --exclude=.gitignore \
      --exclude=README.md \
      "$COMMITTED_DIR" "$TMP_OUT" >/dev/null; then
    echo "OK: bindings match the current WASM (hash $WASM_HASH)."
    exit 0
  else
    echo "DRIFT: $COMMITTED_DIR does not match the freshly generated bindings." >&2
    echo "       Regenerate with: scripts/gen-bindings.sh" >&2
    echo "       Full diff follows:" >&2
    diff -r \
      --exclude=node_modules \
      --exclude=dist \
      --exclude=.gitignore \
      --exclude=README.md \
      "$COMMITTED_DIR" "$TMP_OUT" >&2 || true
    exit 1
  fi
fi

# Generation mode: replace the committed tree, preserving the in-repo
# README.md and .gitignore that live alongside it.
echo "==> replacing $COMMITTED_DIR with the generated output"
# Preserve the in-repo README.md (which documents *this directory's
# purpose*, not the emitted package's own README) and .gitignore.
SAVED_README=""
SAVED_GITIGNORE=""
if [[ -f "$COMMITTED_DIR/README.md" ]]; then
  SAVED_README="$(mktemp)"
  cp "$COMMITTED_DIR/README.md" "$SAVED_README"
fi
if [[ -f "$COMMITTED_DIR/.gitignore" ]]; then
  SAVED_GITIGNORE="$(mktemp)"
  cp "$COMMITTED_DIR/.gitignore" "$SAVED_GITIGNORE"
fi

rm -rf "$COMMITTED_DIR"
mkdir -p "$COMMITTED_DIR"
# Copy the generated contents in — `cp -R "$TMP_OUT"/. "$COMMITTED_DIR/"`
# preserves hidden files (notably `.wasm-sha256`) which plain
# `cp -R "$TMP_OUT"/* ...` would skip on default shells.
cp -R "$TMP_OUT"/. "$COMMITTED_DIR/"

if [[ -n "$SAVED_README" ]]; then
  cp "$SAVED_README" "$COMMITTED_DIR/README.md"
  rm -f "$SAVED_README"
fi
if [[ -n "$SAVED_GITIGNORE" ]]; then
  cp "$SAVED_GITIGNORE" "$COMMITTED_DIR/.gitignore"
  rm -f "$SAVED_GITIGNORE"
fi

echo "OK: $COMMITTED_DIR regenerated from WASM $WASM_HASH"
echo "    Review the diff, then commit (the .wasm-sha256 file pins which WASM this came from)."
