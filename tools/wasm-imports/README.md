# `wasm-imports` — scan the release WASM for host capabilities

`contracts/escrow`'s "no admin, no upgrade, no deployer" guarantee
([`docs/threat-model.md`](../../docs/threat-model.md) entry 26) is a claim
about the **compiled artifact**, not about the source tree. A source review can
show no `pub fn` that upgrades the contract, while a dependency, a macro
expansion, or a stray `extern` declaration still links
`update_current_contract_wasm` or a deployer function into the WASM.

This crate reads the import section of the release WASM that
`stellar contract build` produces, resolves each short import name back to its
Soroban host function using a vendored copy of `soroban-env-common`'s
`env.json`, and:

1. prints the full import list, one host function per line;
2. **fails on denylisted capabilities** (upgrade, upload, and deploy paths);
3. **fails when the import list drifts from the committed snapshot**
   (`imports.snapshot`), so any new host capability shows up as a reviewable
   diff even before it is denylisted.

It is a standalone crate (`[workspace]` in its `Cargo.toml`), deliberately not
a member of the contract workspace: the guard must not change what
`stellar contract build` compiles.

## Why a snapshot as well as a denylist

The denylist answers "did we link a known-bad capability?". The snapshot
answers "did the artifact's capability surface change at all?" — a new host
import that is not on today's denylist (a future `update_*`, a new storage or
crypto host function, a dependency's `get_ledger_*` read) still appears in the
PR diff and has to be reviewed on purpose. `#254`'s framing: the interface
snapshot catches new entry points; this catches new capabilities.

## Usage

Prerequisites: Rust (the repo's pinned toolchain) and `stellar-cli`, which the
SDK requires for a WASM build.

```bash
# From the repository root.
stellar contract build

# Print imports, check the denylist, and compare with the snapshot.
cargo run --manifest-path tools/wasm-imports/Cargo.toml

# Point at a different build (e.g. a deployed artifact).
cargo run --manifest-path tools/wasm-imports/Cargo.toml -- \
  --wasm path/to/harvestlock_escrow.wasm --snapshot /tmp/imports.snapshot

# Regenerate the committed snapshot after an intentional, reviewed change.
cargo run --manifest-path tools/wasm-imports/Cargo.toml -- --update

# Just print, e.g. to inspect a third-party WASM.
cargo run --manifest-path tools/wasm-imports/Cargo.toml -- --print-only
```

Defaults: `--wasm target/wasm32v1-none/release/harvestlock_escrow.wasm` and
`--snapshot tools/wasm-imports/imports.snapshot`. Exit codes: `0` clean, `1`
denylisted function or snapshot drift, `2` usage/IO/parse error.

Current output for the release build (also the committed snapshot):

```
# module:import  host function
a:0  require_auth
b:8  bytes_len
b:j  symbol_new_from_linear_memory
b:m  symbol_index_in_linear_memory
d:_  call
i:0  obj_to_u64
i:6  obj_from_i128_pieces
i:7  obj_to_i128_lo64
i:8  obj_to_i128_hi64
i:_  obj_from_u64
l:0  has_contract_data
l:1  get_contract_data
l:8  extend_current_contract_instance_and_code_ttl
l:_  put_contract_data
m:9  map_new_from_linear_memory
m:b  sparse_map_new_from_linear_memory
m:c  sparse_map_unpack_to_linear_memory
v:1  vec_get
v:3  vec_len
v:6  vec_push_back
v:_  vec_new
v:g  vec_new_from_linear_memory
x:0  obj_cmp
x:1  contract_event
x:3  get_ledger_sequence
x:4  get_ledger_timestamp
x:7  get_current_contract_address
x:8  get_max_live_until_ledger
```

No `update_*`, `upload_wasm`, or `create_*` capability appears. The other
imports are the expected Soroban surface: auth (`require_auth`), storage
(`get/put/has_contract_data`, TTL extension), cross-contract calls (`call`),
events, ledger time/sequence, and value/collection encoding helpers.

## The denylist

| Host function | Why it must never link |
| --- | --- |
| `update_current_contract_wasm` | upgrades the running contract's code — an upgrade key by another name |
| `update_current_contract_executable_ref` | points the running contract at different executable code |
| `upload_wasm` | installs new contract code into the ledger |
| `create_contract` | deploys a contract from this one (a deployer capability) |
| `create_contract_with_constructor` | deploys a contract from this one |
| `create_asset_contract` | deploys or wraps an asset contract from this one |
| `create_external_ref_contract` | deploys an external-reference contract from this one |

A new entry needs a reason and a test; `DENY_LIST` in `src/lib.rs` is checked
against the vendored table by
`every_denylisted_name_exists_in_the_vendored_table`, so a typo cannot silently
disarm an entry.

The test `a_wasm_that_upgrades_itself_fails_the_denylist` assembles a tiny WAT
module importing `env` `l6` (`update_current_contract_wasm`) and asserts the
scan fails; `a_deployer_import_is_denied_too` does the same for `l3`
(`create_contract`). No second contract build is needed.

## The vendored `env.json`

`data/env.json` is copied verbatim from `soroban-env-common` 29.0.1 (the
version `soroban-sdk = 29.0.1` resolves to). It maps each host module's export
prefix and each function's export suffix to the function name — the WASM
import module is the prefix (`l`), the field is the suffix (`_`), so the short
name is `l_` → `put_contract_data`.

Refresh it when the SDK is upgraded:

```bash
curl -fsSL \
  https://raw.githubusercontent.com/stellar/rs-soroban-env/v29.0.1/soroban-env-common/env.json \
  -o tools/wasm-imports/data/env.json
```

An import the vendored file does not know is **not dropped**: the tool prints
`<unmapped>`, warns on stderr, and still records it in the snapshot, so a new
SDK's capability is visible even before `env.json` is refreshed.

## What this guard does and does not prove

- It proves the release WASM does not **directly import** the listed
  capabilities. Soroban's capability model is import-based: a contract can only
  invoke host functions it links.
- It does not audit what cross-contract `call` targets do (the escrow makes
  token calls by design). It also cannot stop a maintainer from editing the
  denylist or `env.json` in the same PR — the snapshot makes that visible in
  review, which is the enforcement.
- It scans imports, not exports or the contract spec; the exported ABI has its
  own snapshot in `docs/0002-interface-spec.md`.

## CI wiring (maintainer-managed)

Per [`.github/PULL_REQUEST_TEMPLATE.md`](../../.github/PULL_REQUEST_TEMPLATE.md),
contributors do not edit workflows. This PR ships the tool, its tests, the
snapshot, and this document. The maintainers can wire it in after
`stellar contract build` in `.github/workflows/test.yml`'s `build-wasm` job:

```yaml
      - name: Guard the WASM's host imports
        run: |
          cargo test --manifest-path tools/wasm-imports/Cargo.toml
          cargo run --manifest-path tools/wasm-imports/Cargo.toml
```

The first line needs no deployment and covers the denylist/snapshot logic with
synthetic WAT fixtures; the second checks the actual release artifact. Both
share the build job's `target/` and need no extra setup.
