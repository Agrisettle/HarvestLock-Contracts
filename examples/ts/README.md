# Collecting multi-party auth for `cancel`

`cancel` on HarvestLock requires the buyer **and** the cooperative:

```rust
Self::can_cancel(&c)?;
Self::require_once_each(&[&c.buyer, &c.cooperative]);
```

`require_once_each` calls `require_auth()` once per *distinct* address
(`contracts/escrow/src/lib.rs`). If the same address holds both roles, it signs
once. This is issue #139/#140, and it is where hand-written clients usually go
wrong: they either ask for two signatures when one is enough, or they sign both
auth entries with one keypair when the roles are actually distinct.

This example shows the correct pattern with the generated TypeScript bindings
and `@stellar/stellar-sdk`:

1. simulate the call with the generated `Client`,
2. ask `tx.needsNonInvokerSigningBy()` which addresses still need auth,
3. call `tx.signAuthEntries({ address, signAuthEntry: keypair })` once per
   address, using that address's own keypair,
4. `tx.signAndSend()` to submit.

It also runs `lock` first as the single-signer contrast.

A **third** funded identity (`INVOKER_SECRET`) is the transaction source. The
source's signature covers the envelope, so all `require_auth()` addresses show
up as non-invoker auth entries. If you used the buyer as the source, the
buyer's entry would be hidden behind the envelope signature and the example
would not demonstrate the multi-party collection.

## Prerequisites

- Node.js 20+ and npm
- Rust toolchain and `stellar` CLI 25.2+ (bindings were generated with 28.1.0)
- Three funded Stellar **testnet** identities: `buyer`, `cooperative`, `invoker`

Fund fresh identities with the testnet friendbot:

```sh
for name in buyer cooperative invoker; do
  stellar keys generate "$name" --network testnet
  stellar keys fund "$name" --network testnet
done

export BUYER_SECRET=$(stellar keys secret buyer)
export COOPERATIVE_SECRET=$(stellar keys secret cooperative)
export INVOKER_SECRET=$(stellar keys secret invoker)
```

For the shared-address run, also export `COOPERATIVE_SECRET="$BUYER_SECRET"`
and initialize the contract with `--cooperative "$BUYER"` (below).

## 1. Build, deploy, initialize

From the repository root:

```sh
stellar contract build --locked
WASM_HASH=$(stellar contract install \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account invoker --network testnet)

BUYER=$(stellar keys address buyer)
COOP=$(stellar keys address cooperative)
WAREHOUSE=$(stellar keys address invoker)

# Instance A — distinct buyer and cooperative.
CONTRACT_ID=$(stellar contract deploy --wasm-hash "$WASM_HASH" \
  --source-account invoker --network testnet)

stellar contract invoke --id "$CONTRACT_ID" --source-account buyer --network testnet --send yes -- \
  initialize \
  --buyer "$BUYER" --cooperative "$COOP" --warehouse_operator "$WAREHOUSE" \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --total_amount 100000000 --advance1_bps 1500 --advance2_bps 2000 \
  --claim_window_secs 3600 --remainder_window_secs 3600 --delivery_window_secs 86400 \
  --contracted_quantity 1000 --grade_price_bps '[10000,9000,7500]' --oracle_config null
```

`lock` pulls the buyer's deposit in the native-XLM SAC token
(`CDLZFC3S…`, the same token used in `docs/gas-costs.md`), so the buyer must
hold a small balance of it. Friendbot-funded accounts on testnet already do.

## 2. Run the example

The generated bindings are committed under `bindings/`. `npm start` rebuilds
them from `bindings/src/index.ts` and then runs the example with `tsx`:

```sh
cd examples/ts
cp .env.example .env   # optional; or just export the variables above
npm install
CONTRACT_ID="$CONTRACT_ID" npm start
```

Expected output for distinct roles:

```
distinct addresses: require_once_each requires two signatures

1) lock — single signer
  needs auth: buyer
  lock tx: <hash>

2) cancel — multi-party, deduplicated by address
  needs auth: buyer, cooperative
  cancel tx: <hash>
```

`signerCount` in the final JSON is `2`.

## 3. Shared-address case

Deploy a second instance with the buyer holding both named roles:

```sh
SHARED_CONTRACT_ID=$(stellar contract deploy --wasm-hash "$WASM_HASH" \
  --source-account invoker --network testnet)

stellar contract invoke --id "$SHARED_CONTRACT_ID" --source-account buyer --network testnet --send yes -- \
  initialize \
  --buyer "$BUYER" --cooperative "$BUYER" --warehouse_operator "$WAREHOUSE" \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --total_amount 100000000 --advance1_bps 1500 --advance2_bps 2000 \
  --claim_window_secs 3600 --remainder_window_secs 3600 --delivery_window_secs 86400 \
  --contracted_quantity 1000 --grade_price_bps '[10000,9000,7500]' --oracle_config null

cd examples/ts
COOPERATIVE_SECRET="$BUYER_SECRET" CONTRACT_ID="$SHARED_CONTRACT_ID" npm start
```

This time the script logs
`shared address: require_once_each collapses both roles into one signature`,
`needs auth: buyer` for **both** `lock` and `cancel`, and `signerCount: 1`.
The single auth entry is signed by the buyer's keypair only.

## Verified on testnet

Both paths were run against `https://soroban-testnet.stellar.org` with three
friendbot-funded identities and the WASM built from this branch. Every hash
below returned `status: SUCCESS` from `getTransaction`.

| Case | Contract | `lock` tx | `cancel` tx | signer count |
| --- | --- | --- | --- | --- |
| Distinct buyer/cooperative | `CADWZ2Y4QXZRELVNSTQAEZKMMJWCNKWVNM7BEZJ34O5GSZAXLHOXURWJ` | `83ec263b75d19d08002f22d4426a01ed2c444bb1ec20cf4e04bbc75bd4dd2009` | `f21931d6b8061c80fdc8d39b65569b50872c8768007b4d28ff370c4f429c04cb` | 2 |
| Shared address (buyer == cooperative) | `CA2PDDIZVBEUHCLEEF3NRYT4ODTBOC3K2NXV6AIL4QXWLCBNX7M5OWQX` | `bc8fede1d71082e5d2afb6b768b6c3bdd607e6dbab27189293b4e1e75d821d66` | `eaaaab93652da87476af0cfe25eb5f30340065c708fb3a10d3cb237a4e68eada` | 1 |

The shared-address `cancel` has one auth entry and one signature even though
two named roles required consent — that is `require_once_each` deduplicating
(#139/#140), observed through `needsNonInvokerSigningBy()`.

## Regenerating the bindings

After changing the contract, regenerate from the repo root:

```sh
stellar contract build --locked
stellar contract bindings typescript \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --output-dir examples/ts/bindings --overwrite
```
