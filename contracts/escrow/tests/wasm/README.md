# `tests/wasm/` — third-party contract binaries, pinned by hash

Contracts this crate calls but does not build. Each file is the exact bytes
deployed on a public network. A test imports it with
`soroban_sdk::contractimport!(file = …, sha256 = …)`, which fails the
*build* if the file's hash ever differs from the pinned one, so a swapped or
corrupted binary cannot silently change what the tests measure.

## `reflector_fiat_testnet.wasm`

| | |
| --- | --- |
| What | Reflector "Fiat exchange rates" oracle (Reflector's `pulse-contract`) |
| Network | Stellar testnet |
| Contract | `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W` |
| WASM sha256 | `25fc306065cbb9b05461db78ec259802cb916053ba5b3ab911b0a72dd1a81439` |
| Size | 20,417 bytes |
| Code entry last modified | ledger 5,162,610 |
| Fetched | 2026-11-25, testnet ledger ~5,914,834 |
| Embedded meta | `cliver = 28.1.0`, `source_repo = github:reflector-network/reflector-contract`, env meta protocol 28 |
| Used by | `tests/reflector_oracle_costs.rs` (issue: measure the real FX path cost) |

**How it was fetched.** This is what `stellar contract fetch --id
CCSSOHTB…NV4W --network testnet` does. That CLI was not installed in the
fetching environment, so the same two RPC lookups were made by hand against
`https://soroban-testnet.stellar.org`:

1. `getLedgerEntries` for the contract's instance key
   (`LedgerKey::ContractData { contract, key: LedgerKeyContractInstance,
   durability: Persistent }`), giving the `ContractExecutable::Wasm` hash;
2. `getLedgerEntries` for `LedgerKey::ContractCode { hash }`, giving the code.

The sha256 of the returned code was checked against the on-chain hash
before saving.

**Re-verify** (needs `stellar-cli`):

```sh
stellar contract fetch --network testnet \
  --id CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W -o /tmp/r.wasm
shasum -a 256 /tmp/r.wasm contracts/escrow/tests/wasm/reflector_fiat_testnet.wasm
```

If the hashes differ, Reflector has upgraded the contract (it has an
admin-only `update_contract`). Replace the file, update the `sha256` in the
test, re-run it, and refresh the oracle group in `docs/gas-costs.md`. Do not
just update the hash.

**Source caveat.** The embedded commit (`c0f4d0da…`) is not in the public
`reflector-contract` history (it was probably squashed). The interface was read
from the WASM's own `contractspecv0` section, and the behaviour from the public
source at `b006d29`, whose `pulse-contract` exports the identical function set.
The test exercises the binary itself, so it does not depend on that source
match.
