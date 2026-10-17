# Gas costs — advance-tranche methods, measured on Stellar testnet

Real resource-cost numbers for the six advance-tranche methods
(`release_advance_1`/`_2`, `claim_advance_1`/`_2`, `reclaim_advance_1`/`_2`),
measured with
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
against freshly-deployed testnet instances of this contract. This is the
**measured record** companion to [`cost-estimation.md`](./cost-estimation.md),
which covers the separate question of *which tool* to standardize on — this doc
doesn't re-argue that, it just reports what the estimator actually returned.

## What was measured, and against what

| | |
| --- | --- |
| Tool | `soroban-cost-estimator` **v0.1.0** (crates.io), `estimate --wasm … --id … --fn …` |
| Network | Stellar **testnet** (`https://soroban-testnet.stellar.org`), **protocol 29** |
| When | 17 Oct 2026, across ledgers ~5243848–5243875 (each row is one `simulateTransaction` RPC at that moment) |
| WASM | `harvestlock_escrow.wasm`, hash `39b99c184c943b29707d9bf65ee4c34bcd2ae2704e37e805b1183c8eca03a14b`, 52,119 bytes, built from this branch's HEAD via `stellar contract build` |
| Instance A (claim path) | `CDL3RTC6A52B4XJTD7YQ4SWL6RVLZQ44NMOSYC2ENIR3DPKEINB7RC2Q` ([stellar.expert](https://stellar.expert/explorer/testnet/contract/CDL3RTC6A52B4XJTD7YQ4SWL6RVLZQ44NMOSYC2ENIR3DPKEINB7RC2Q)) — `claim_window_secs: 3600` |
| Instance B (reclaim path) | `CCXI4MFIHALCMFPX2Y3G5V2WMBCXR46JDWLUW7KBYXCR6WJQQQ2MATUC` ([stellar.expert](https://stellar.expert/explorer/testnet/contract/CCXI4MFIHALCMFPX2Y3G5V2WMBCXR46JDWLUW7KBYXCR6WJQQQ2MATUC)) — `claim_window_secs: 15` (demo-only, same reasoning as HANDOFF's Deployments 2/5) |
| Commitment shape | native XLM SAC (`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`), `total_amount: 100,000,000` stroops, 15%/20% advance split |

**Why two instances, not one**: `claim_advance_*` requires the claim window to
still be *open*, `reclaim_advance_*` requires it to have *passed* — mutually
exclusive in wall-clock time on a single instance. Each instance was walked to
the required on-chain state with real transactions before the corresponding
simulations were run (a simulation against the wrong state just errors — the
contract's guards are enforced in the simulator too, which is the point of
measuring against a deployed instance rather than a local harness). All six
functions take no arguments, so no `--arg` was needed.

## Results

One row per method. "Fee" columns are stroops (1 XLM = 10,000,000 stroops);
`Fee (total)` is the estimator's non-refundable + refundable sum — read the
caveats section before treating it as "what a transaction costs".

| Method | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) | Fee (total XLM) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `release_advance_1` | 936,397 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,341 | 13,233 | 15,574 | 0.0015574 |
| `release_advance_2` | 939,162 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,342 | 6,456 | 8,798 | 0.0008798 |
| `claim_advance_1` | 1,611,598 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `claim_advance_2` | 1,611,619 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `reclaim_advance_1` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |
| `reclaim_advance_2` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |

## ⚠️ Flag — claim/reclaim fees look ~54–96× a release, but 99.2% of that is simulated storage rent, not execution

`claim_advance_*`/`reclaim_advance_*` each report ~846.9k stroops against a
release's 8.8k–15.6k — but the refundable (rent) half is 839.8k of it, and a
**real submitted** `reclaim_advance_2` on the same instance was charged
**8,189 stroops** total (4,908 of the proposed fee refunded); the
non-refundable/execution half the estimator reports (~7.1k) is the comparable
number, and it's in the same league as a release. The disproportionate part is
the storage-rent assumption baked into the simulation's footprint (4 written
entries: contract instance + SAC balance entries, extended to the simulation's
TTL horizon), not anything the contract's logic burns — worth remembering if
footprint entries ever multiply, not an active problem today.

## Reading the numbers

- **The releases are cheap and nearly identical** (~939k CPU, 1 write entry —
  the contract instance). They only move a deadline in instance storage. The
  entire 15,574-vs-8,798 stroop gap between `release_advance_1` and
  `release_advance_2` sits in the *refundable* half (13,233 vs 6,456); the
  execution halves differ by 1 stroop. That's a fresh-instance TTL artifact of
  when the simulation ran, not a difference in contract logic — the two
  functions share one private `open_tranche` implementation by design (see
  HANDOFF's design decisions).
- **The claims and reclaims are all the same cost within noise** (~1.610–1.612M
  CPU, 2 read / 4 write entries): tranche 1 and 2 route through the same
  private `claim_tranche`/`reclaim_tranche`, and claim-vs-reclaim only flips a
  deadline comparison. The extra CPU over a release is the SEP-41 SAC
  `transfer` cross-contract call (and its events) that releases don't make.
- **Memory bytes = 0 on every row** — the tool found no memory-bytes figure in
  the current-protocol `simulateTransaction` response to decode; use CPU
  instructions as the compute metric. It is *not* a claim that these calls use
  no memory.
- **The WASM is 52,119 bytes here** vs Deployment 10's 47,171 optimized — this
  build includes the `status_changed` event surface shipped after that
  deployment, and `stellar contract optimize` is a no-op on this toolchain
  (same byte count in and out).

## Cross-check against the CLI (the source of record, per `cost-estimation.md`)

`reclaim_advance_2` on instance B was actually **submitted** (not just
simulated) with `stellar contract invoke … --cost`, as an independent check on
the estimator's fee math — tx
[`039a3d1c…`](https://stellar.expert/explorer/testnet/tx/039a3d1c278429d43867cdb0567ca372c42aa8cc8ca6accab26f0a1c01579a58):

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 7,105 | 7,620 charged |
| Refundable / rent | 839,778 proposed-in-footprint | 5,377 proposed → 469 charged, 4,908 refunded |
| Total resource fee | 846,883 | 8,089 charged |
| Inclusion fee | — | 100 charged |

The ~515-stroop non-refundable difference is envelope shape (the CLI's real
transaction carries auth/footprint details the estimator's synthetic envelope
doesn't); the rent gap is the story flagged above. Note that
`stellar contract invoke --send=no --cost` prints **no** cost block on
stellar-cli v28 — cost only prints on a submitted transaction.

## Reproduce

```bash
# 1. Build + upload + deploy two fresh instances (from repo root)
stellar contract build
WASM_HASH=$(stellar contract install --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet)
A=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)
B=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)

# 2. Initialize both (identical except the claim window) as buyer, then lock.
#    claim window 3600 on A (claims stay open), 15 on B (windows lapse fast).
stellar contract invoke --id "$A" --source-account buyer --network testnet -- \
  initialize --buyer <buyer G…> --cooperative <coop G…> --warehouse-operator <warehouse G…> \
  --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
  --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000 \
  --claim-window-secs 3600 --remainder-window-secs 3600 --delivery-window-secs 86400 \
  --contracted-quantity 1000 --grade-price-bps '[10000,9000,7500]' --oracle-config null
# … same for "$B" with --claim-window-secs 15, then on each:
stellar contract invoke --id <id> --source-account buyer --network testnet -- lock

# 3. Measure on A (each estimate is a simulation only — state advances only via invoke)
soroban-cost-estimator estimate --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn release_advance_1 --network testnet          # while Locked
stellar contract invoke --id "$A" --source-account deployer --network testnet -- release_advance_1
soroban-cost-estimator estimate --wasm … --id "$A" --fn claim_advance_1 --network testnet  # window open
stellar contract invoke --id "$A" --source-account warehouse --network testnet -- mark_checkpoint
soroban-cost-estimator estimate --wasm … --id "$A" --fn release_advance_2 --network testnet # while CheckpointPassed
stellar contract invoke --id "$A" --source-account deployer --network testnet -- release_advance_2
soroban-cost-estimator estimate --wasm … --id "$A" --fn claim_advance_2 --network testnet  # window open

# 4. Measure on B: after release_advance_1, wait out the 15 s window, then
#    estimate reclaim_advance_1 (status still Advance1Released, window passed);
#    mark_checkpoint, release_advance_2, wait again, estimate reclaim_advance_2.
```

## Caveats

- **Estimator version is v0.1.0**, the crates.io release current at this
  writing — the upstream README describes newer flags (`--compare`, snapshot
  retention) that this version doesn't have. The `estimate` path used here is
  stable across both.
- **`Fee (total)` is an upper bound**, not the charged fee — it includes
  simulated TTL-extension rent that a real submission largely gets refunded or
  never pays (see the cross-check). For budgeting, use the non-refundable
  column plus the CLI's real-submission numbers.
- **Both instances are validation artifacts**, per HANDOFF's standing rule —
  and they've now been walked past these states (B's tranche 2 was genuinely
  reclaimed as the cross-check above). Re-running the estimates against *these*
  IDs will hit state guards and error; deploy fresh per the reproduce steps.
- The SAC balance entries these claims/reclaims touched were created moments
  earlier by `lock` on the same instances. A nth-claim in a long-lived
  commitment may differ in the rent half; the execution half is state-size
  independent here.
## WASM Upload vs Instantiation

The deploy flow separates two distinct costs:

| Operation | Description | Cost (stroops) | Notes |
|-----------|-------------|----------------|-------|
| **WASM upload** | One-time upload of the compiled `.wasm` to the Soroban network | ~TBD | Paid once per contract version; hash pinned in deployment config |
| **Instantiation** | Creating a new commitment instance from an already-uploaded WASM hash | ~TBD | Marginal cost per commitment; scales with initial state size |

To reproduce these measurements locally:

```bash
# Upload cost (no --fn flag — measures pure upload)
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm

# Instantiation cost (with --id pointing to the deployed contract address)
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --contract-id <DEPLOYED_CONTRACT_ID> \
  --fn initialize \
  --args '["<BUYER>", "<SELLER>", 100000, 5000, 3000, 100, 200, 300, null, []]'
```

## Reproducibility

WASM builds are verified reproducible in CI (`.github/workflows/test.yml`, job "Verify WASM build reproducibility"). The SHA-256 hash of `target/wasm32v1-none/release/harvestlock_escrow.wasm` must match between two consecutive clean builds. Current hash: see latest CI run.

## Supply-Chain Health

`cargo deny check advisories bans` runs in CI to catch yanked or unmaintained dependencies. See `deny.toml` for configuration. As of this writing, the dependency tree is minimal (`soroban-sdk = "28.0.0"` only) and passes all checks cleanly.
