# HarvestLock TTL keeper

`ttl-keeper.ts` checks the persistent contract-instance and Wasm-code entries
for a list of Soroban contract IDs. It uses `getLedgerEntries` to read each
entry's `liveUntilLedgerSeq`. Contracts with either entry below the threshold
are extended with `stellar contract extend` and read again to confirm that both
entries moved forward.

The tool is intentionally not a contract call: RPC reads do not extend TTL.
Real runs therefore require a funded Stellar CLI identity.

## Install

From this directory:

```sh
npm install
```

Node.js 18 or newer is required. The package uses the Stellar SDK for XDR and
RPC handling; the `stellar` CLI is still required for a real run.

## Dry run

Pass IDs directly, or put one ID per line in a file. Blank lines and `#`
comments in the file are ignored.

```sh
npx tsx ttl-keeper.ts --dry-run \
  C...first-contract \
  C...second-contract

npx tsx ttl-keeper.ts --dry-run --file contracts.txt
```

The default RPC endpoint is Stellar testnet. Override it with
`--rpc-url` or `STELLAR_RPC_URL`. The default threshold is 100,000 ledgers.

## Real run

Use a funded CLI identity as `STELLAR_SOURCE`; do not put its secret key in
the contract list or in source control.

```sh
$env:STELLAR_SOURCE = "ttl-keeper"
npx tsx ttl-keeper.ts --file contracts.txt \
  --threshold 100000 \
  --ledgers-to-extend 500000
```

On POSIX shells, use `export STELLAR_SOURCE=ttl-keeper` instead. The tool
passes `--source`, `--network testnet`, `--durability persistent`, and the
configured `--ledgers-to-extend` to the CLI. Set `STELLAR_NETWORK` when using
a configured network other than testnet.

The command exits non-zero if an ID cannot be read, an extension command
fails, or the post-extension read does not show both entries increasing.
`--dry-run` never invokes the CLI.
