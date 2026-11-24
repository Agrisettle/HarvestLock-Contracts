#!/usr/bin/env bash
# Guards the escrow contract's exported interface (#244).
#
# Builds the WASM, decodes its contract spec (function names, argument names
# and types, return types, events, and every user-defined type they reach),
# and compares that with the committed contracts/escrow/interface.snapshot.
# Any difference fails with a unified diff and a version hint under
# docs/versioning.md §2:
#
#   PATCH               interface unchanged (state-machine changes can still
#                       make the release MINOR; the script can't see those)
#   MINOR               purely additive: a new function, event or type, or a
#                       new case on an existing enum/union/error
#   MINOR **BREAKING**  anything removed or changed: a signature, a return
#                       type, a struct field, an enum value
#
# Why it exists: docs/threat-model.md entry 26 says admin, upgrade and pause
# are "defended by absence", and the only guard was reviewers noticing a PR
# that adds one. With the snapshot, a new `pub fn` cannot get in without a
# visible diff to this file. Same pattern as check_error_discriminants.sh.
#
# Usage (from the repo root):
#   bash .github/scripts/check_interface_snapshot.sh            # check
#   bash .github/scripts/check_interface_snapshot.sh --update   # accept change
#
# Environment:
#   WASM=<path>    check this already-built WASM instead of building one
#                  (CI's build-wasm job can pass the WASM it just built)
#   PYTHON=<exe>   Python 3 interpreter (default: python3, then python)
#
# Needs: Python 3, plus the Stellar CLI when WASM is not given. soroban-sdk 29
# refuses a plain `cargo build --target wasm32v1-none` and requires
# `stellar contract build`, so that is what builds the WASM here. The spec is
# decoded by interface_snapshot.py, not by `stellar contract info interface`,
# so the snapshot's format doesn't change when the CLI's output format does.
set -euo pipefail

SNAP="contracts/escrow/interface.snapshot"
MANIFEST="contracts/escrow/Cargo.toml"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

mode="check"
case "${1:-}" in
  "") ;;
  --update) mode="update" ;;
  -h|--help) sed -n '2,38p' "$0"; exit 0 ;;
  *) echo "error: unknown argument '$1' (expected --update)" >&2; exit 2 ;;
esac

if [[ ! -f "$MANIFEST" ]]; then
  echo "error: $MANIFEST not found (run from repo root)" >&2
  exit 1
fi

if [[ -z "${PYTHON:-}" ]]; then
  if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON=python3
  else
    PYTHON=python
  fi
fi

if [[ -z "${WASM:-}" ]]; then
  if ! command -v stellar >/dev/null 2>&1; then
    echo "error: the Stellar CLI (\`stellar\`) is not on PATH" >&2
    echo "hint: install it (docs/dev-setup-troubleshooting.md), or build the WASM" >&2
    echo "      yourself and pass WASM=target/wasm32v1-none/release/harvestlock_escrow.wasm" >&2
    exit 1
  fi
  echo "building escrow WASM (stellar contract build --locked)..." >&2
  stellar contract build --locked >&2
  WASM="target/wasm32v1-none/release/harvestlock_escrow.wasm"
fi
if [[ ! -f "$WASM" ]]; then
  echo "error: WASM not found at $WASM" >&2
  exit 1
fi

if [[ "$mode" == "update" ]]; then
  exec "$PYTHON" "$HERE/interface_snapshot.py" update "$WASM" "$SNAP"
fi
if [[ ! -f "$SNAP" ]]; then
  echo "error: $SNAP not found; generate it with --update" >&2
  exit 1
fi
exec "$PYTHON" "$HERE/interface_snapshot.py" check "$WASM" "$SNAP"
