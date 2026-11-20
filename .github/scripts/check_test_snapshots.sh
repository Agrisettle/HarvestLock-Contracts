#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

cargo test --locked --manifest-path contracts/escrow/Cargo.toml

snapshot_dir="contracts/escrow/test_snapshots"
git diff --exit-code HEAD -- "$snapshot_dir"

untracked="$(git ls-files --others --exclude-standard -- "$snapshot_dir")"
if [[ -n "$untracked" ]]; then
    printf 'error: cargo test created untracked snapshots:\n%s\n' "$untracked" >&2
    exit 1
fi

echo "ok: test snapshots are unchanged"
