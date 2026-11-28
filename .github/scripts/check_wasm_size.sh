#!/usr/bin/env bash
set -euo pipefail

WASM="target/wasm32v1-none/release/harvestlock_escrow.wasm"
BASELINE="contracts/escrow/wasm_size.baseline"
MAX_GROWTH_BYTES=5120

if [[ ! -f "$WASM" ]]; then
  echo "error: $WASM not found (run \`stellar contract build --locked\` from the repository root)" >&2
  exit 1
fi
if [[ ! -f "$BASELINE" ]]; then
  echo "error: $BASELINE not found" >&2
  exit 1
fi

baseline=$(<"$BASELINE")
if [[ ! "$baseline" =~ ^[0-9]+$ ]] || (( baseline == 0 )); then
  echo "error: $BASELINE must contain one positive byte count" >&2
  exit 1
fi

current=$(wc -c <"$WASM" | tr -d '[:space:]')
delta=$((current - baseline))

printf 'WASM size budget: baseline=%s bytes current=%s bytes delta=%+d bytes (growth limit=%s bytes)\n' \
  "$baseline" "$current" "$delta" "$MAX_GROWTH_BYTES"

if (( delta > MAX_GROWTH_BYTES )); then
  echo "error: WASM grew beyond the ${MAX_GROWTH_BYTES}-byte budget; review the change and update $BASELINE in the same PR if intentional" >&2
  exit 1
fi
