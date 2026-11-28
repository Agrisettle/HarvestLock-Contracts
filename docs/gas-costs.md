# Gas costs — measured on Stellar testnet with `soroban-cost-estimator`

Real resource-cost numbers for this contract's public methods, measured with
[`soroban-cost-estimator`](https://github.com/Stellar-Cost-Labs/soroban-cost-estimator)
against freshly-deployed testnet instances. This is the **measured record**
companion to [`cost-estimation.md`](./cost-estimation.md), which covers the
separate question of *which tool* to standardize on — this doc doesn't
re-argue that, it just reports what the estimator actually returned.

## Fee-bump sponsorship

[`scripts/fee-bump-submit.mjs`](../scripts/fee-bump-submit.mjs) demonstrates
submitting `claim_advance_1` or `cancel` on Testnet with a separate fee payer.
Install its pinned SDK dependency with `npm ci --prefix scripts` (Node.js
22.12 or newer), then set `INNER_SOURCE_SECRET` and `SPONSOR_SECRET` in the
environment. The inner source must be funded and have a sequence number; for
`claim_advance_1` it must be the cooperative. For `cancel`, use the buyer as
the inner source and set `OTHER_PARTY_SECRET` to the cooperative's secret.
The script signs
non-source Soroban authorization entries with the matching party key, signs
the inner transaction with its source, and only then wraps it in a fee-bump
transaction signed by the sponsor. The sponsor must be a distinct, funded
account. Secrets are read from the environment and are never printed.

Run, for example:

```sh
node scripts/fee-bump-submit.mjs <contract-id> claim_advance_1
node scripts/fee-bump-submit.mjs <contract-id> cancel
```

The fee-bump sponsor pays the transaction inclusion fee and Soroban resource
fee (including the resource fee's rent component). Sponsorship does not pay
for tokens moved by the contract, remove any account funding or sequence
requirements, alter `require_auth`, or make future storage TTL extensions
free. `claim_advance_1` still requires cooperative authorization; `cancel`
still requires both the buyer and cooperative. The contract must already be
in a state where the selected method succeeds, and the party keys must match
its stored addresses.

Both methods were submitted successfully on Testnet against a freshly deployed,
disposable contract using generated test accounts:

| Method | Fee-bump transaction |
| --- | --- |
| `claim_advance_1` | [d00508ba1eda47077dafb7740a27dcf2e4bc760e0e2d3851ac22b75b4889f390](https://stellar.expert/explorer/testnet/tx/d00508ba1eda47077dafb7740a27dcf2e4bc760e0e2d3851ac22b75b4889f390) |
| `cancel` | [6a1001e06fcec20c2667bfeca0afbfb9b31051cf9718cd344b68d5a9b8d90a7f](https://stellar.expert/explorer/testnet/tx/6a1001e06fcec20c2667bfeca0afbfb9b31051cf9718cd344b68d5a9b8d90a7f) |

Two measurement sessions are recorded here, kept deliberately separate
because they were taken against **different builds**:

| Session | Date | Methods | WASM | SDK |
| --- | --- | --- | --- | --- |
| **1** | 17 Oct 2026 | `release_advance_1`/`_2`, `claim_advance_1`/`_2`, `reclaim_advance_1`/`_2` | 52,119 bytes | 28.0.0 |
| **2** | 20 Oct 2026 | **`mark_checkpoint`, `confirm_delivery`** + same-session controls | 60,043 bytes | 29.0.1 |

Session 2 also re-measured `release_advance_1` and `claim_advance_1` on the
same build as a **control pair**, so the two sessions' numbers can be
compared without guessing at the SDK-version delta — see
[Same-session controls](#same-session-controls).

---

## Event emission by `data_format` (#220)

`#[contractevent]` supports three wire encodings for an event's `data`
payload. Event bytes are a billed resource (the network's
`contract_events_size_bytes` limit is 16,384 bytes per transaction, and rent
is charged per KB by `env.cost_estimate().fee()`'s `fee_per_contract_event_1kb:
5000`), and the format is fixed at the type declaration, so every future
emission of that event pays it. This section measures all three on the two
shapes this contract actually has.

These numbers are **in-process**, from `env.cost_estimate()` against the
test host — not a submitted transaction. They are deterministic and useful
for ranking and for catching a format regression; they are *not* comparable
to the testnet tables above (no WASM instantiation, no rent, no envelope).
Reproduce with:

```bash
cd contracts/escrow
cargo test --test event_cost -- --nocapture
```

### Zero-data-field event (the `StatusChanged` shape)

Two topics, no `data` fields. The only thing `data_format` changes is the
empty container it carries.

| `data_format` | Event bytes | CPU instructions | Memory bytes |
| --- | ---: | ---: | ---: |
| `map` (macro default) | 132 | 17,798 | 6,399 |
| `vec` | 132 | 17,295 | 6,383 |
| **`single-value`** | **128** | **16,733** | **6,306** |

The two topics are `Status` values and are identical in all three rows; so
is the event name. Only the payload tag differs. `single-value` is 4 bytes
and ~1,065 CPU instructions cheaper than the default here.

### Three-data-field event (the `DeliveryConfirmed` shape)

One topic, three data fields.

| `data_format` | Event bytes | CPU instructions | Memory bytes |
| --- | ---: | ---: | ---: |
| `map` (macro default) | 192 | 19,946 | 6,589 |
| **`vec`** | **120** | **15,710** | **6,210** |
| `single-value` | — | — | not legal: the macro requires at most one data field |

`vec` is 72 bytes (37.5%) and ~4,236 CPU instructions smaller. The gap is
the map's per-field name keys (`delivered_quantity`, `settlement_bps`, …),
which `vec` replaces with position.

### The rule this contract follows, and why

- **`single-value` for events with 0 or 1 data field.** It is strictly the
  smallest encoding, loses no information a consumer uses (there is no field
  name to preserve), and is applied to the shipped `StatusChanged` and
  `FxQuoteStaleChanged`. `StatusChanged` shipping at `single-value` measures
  **120 bytes / 4,682 CPU / 503 memory bytes** when published by itself.
- **`map` for events with two or more data fields.** `vec` is measurably
  smaller, but at this scale the saving is not worth surrendering the field
  names: 72 bytes is ~352 stroops per event at the SDK's 5,000-stroop/KB
  snapshot rate, against a 16,384-byte transaction budget — while a
  positional encoding makes every indexer depend on field order, and a
  future field insertion silently reinterprets existing data. Named fields
  keep the event stream self-describing, which is the property
  `tests/event_sourcing.rs` relies on to rebuild state from events alone.
  The default is therefore kept **deliberately**, not by inertia.
- **`single-value` is not available for multi-field events at all** — the
  macro rejects it at compile time, so it was never a candidate for
  `DeliveryConfirmed`, `Payout`, `FxResolved`, or the other multi-field
  events.

The comment above each affected event in `contracts/escrow/src/lib.rs`
points back to this section, so the next event author sees the measurement
before choosing a format.

## Offline regression baseline — `contracts/escrow/tests/budget.rs` (#222)

Everything recorded in this document needs a live RPC and a deployed
instance, so none of it runs in CI. The hermetic half is
[`contracts/escrow/tests/budget.rs`](../contracts/escrow/tests/budget.rs),
which runs inside the ordinary `cargo test` job with no network, no funded
account and no deploy.

It drives **all 34 exported methods** to a state where the call genuinely
succeeds (a rejected call is a cheaper, different code path), then meters
exactly one call per method, with `env.cost_estimate().budget()` reset
immediately before it so scenario setup cannot leak into the figure:

- `budget()` — CPU instructions and peak memory bytes;
- `resources()` — read/write entry counts and byte counts.

The oracle paths (`oracle_rate`, `resolve_fx_shortfall`,
`refresh_fx_quote_status`, `fund_fx_shortfall`,
`expire_fx_shortfall_window`, plus `replace_oracle_config`) run against an
in-process fixed-quote oracle; the multi-party paths (`reassign_buyer`,
`resolve_dispute`, `cancel`) are included too.

The baseline is committed at
[`contracts/escrow/budget.snapshot`](../contracts/escrow/budget.snapshot).
A metric growing by more than **1%** fails the test with a per-method diff:

```text
per-method budget regressed beyond 1% against contracts/escrow/budget.snapshot:
  cancel cpu: 574297 -> 584725 (+1.8%) exceeds 1%
```

The bound is tighter than the 5% the issue suggested on purpose: native host
metering is deterministic (two bless runs are byte-identical), and the
smallest meaningful behavioral regression — one extra read of the stored
`Commitment` — is ~11k instructions, i.e. ~1.9% of the heaviest method. A 5%
bound would let that through on `cancel`, `settle`, `claim_*` and friends.

These are host-model figures for **native Rust** execution under the test
env, not the testnet fees elsewhere in this document. The SDK notes they
understate the WASM-equivalent cost (no VM instantiation, no Wasm reads/rent),
which is fine for a relative guard: every run compares the same harness.

### Re-blessing the baseline

When a change intentionally moves a number, regenerate the snapshot with the
change in hand and review the diff before committing it:

```bash
cd contracts/escrow
BLESS_BUDGET=1 cargo test -p harvestlock-escrow --test budget
```

Then `git diff contracts/escrow/budget.snapshot` — the point is not that the
number is frozen, but that it can no longer move unnoticed. Adding an exported
method also fails the suite until it is added to `PUBLIC_METHODS` and measured;
removing or renaming one leaves a snapshot entry that has no measurement, which
fails too. The suite disables Soroban's `test_snapshots/*.json` capture for its
envs, so this baseline does not add 34 footprint artifacts per run.

---

## Worst-case inputs vs per-transaction limits — `tests/resource_limits.rs` (#224)

`MAX_ALLOCATION_MEMBERS` (200) and `MAX_GRADE_ENTRIES` (16) were chosen from
the PRD's product scale, not from chain limits. This section answers the
question those caps left open: **does a legal commitment at both maxima fit
inside one Soroban transaction, with room to spare?**

### Limits and source

The limits below were read live from testnet's `ConfigSetting` ledger entries
over JSON-RPC (`getLedgerEntries`, key `LedgerKey::ConfigSetting`) on
**2026-11-24**, at latest ledger **5,901,514**. They agree with the SDK's
default `InvocationResourceLimits::mainnet()` snapshot (`soroban-sdk` 29.0.1,
`src/testutils/cost_estimate.rs`, taken 2026-07-10) and with
[Stellar Lab's network limits](https://lab.stellar.org/network-limits). The
test declares them as named constants with this source attached, so a future
network change has to be adopted deliberately rather than silently inherited
from a dependency update.

| Resource | `ConfigSetting` field | Testnet value |
| --- | --- | ---: |
| Instructions | `ConfigSettingContractComputeV0.txMaxInstructions` | 400,000,000 |
| Memory | `ConfigSettingContractComputeV0.txMemoryLimit` | 41,943,040 B (40 MiB) |
| Read entries | `ConfigSettingContractLedgerCostV0.txMaxDiskReadEntries` | 200 |
| Read bytes | `ConfigSettingContractLedgerCostV0.txMaxDiskReadBytes` | 200,000 B |
| Write entries | `ConfigSettingContractLedgerCostV0.txMaxWriteLedgerEntries` | 200 |
| Write bytes | `ConfigSettingContractLedgerCostV0.txMaxWriteBytes` | 132,096 B |
| Transaction size | `ConfigSettingContractBandwidthV0.txMaxBytes` | 132,096 B |
| Ledger entry size | `ConfigSettingContractDataEntrySizeBytes` | 65,536 B |
| Footprint entries | `ConfigSettingContractLedgerCostExtV0.txMaxFootprintEntries` | 400 |

### Measured worst cases

The suite meters exactly one successful top-level call at a time. `initialize`
is measured on an uninitialized instance; `set_allocation` is measured after a
16-grade `initialize`, so the instance entry already carries the schedule; and
`settle` is measured after a full lifecycle on a commitment holding the
200-member allocation **and** the 16 grades. All three are the genuine success
path, not a cheaper rejection.

| Metric | `initialize` (16 grades) | `set_allocation` (200) | `settle` (both) | Limit | Worst used |
| --- | ---: | ---: | ---: | ---: | ---: |
| Instructions | 182,890 | 1,504,073 | 3,515,841 | 400,000,000 | 0.9% |
| Memory bytes | 101,879 | 1,083,790 | 1,600,333 | 41,943,040 | 3.8% |
| Read entries (host memory reads) | 4 | 4 | 5 | 200 | 2.5% |
| Read bytes (restore floor) | 1,624 | 21,668 | 21,668 | 200,000 | 10.8% |
| Write entries | 2 | 2 | 3 | 200 | 1.5% |
| Write bytes | 1,696 | 21,740 | 22,116 | 132,096 | 16.7% |
| Ledger entry bytes | 1,624 | 21,668 | 21,668 | 65,536 | **33.1%** |
| Transaction bytes (modelled) | 1,676 | 21,096 | 1,076 | 132,096 | 16.0% |
| Footprint entries (read + write) | 6 | 6 | 8 | 400 | 2.0% |

The tightest resource is the **ledger entry**, at 21,668 bytes — 33.1% of the
65,536-byte cap, leaving **43,868 bytes** (67% of the cap) of headroom. That
is the same 200-member allocation the existing
`instance_storage_layout_stays_well_below_the_entry_size_limit` test bounds at
50%, so the two checks agree. Instructions and memory are far below their
limits (under 1% and 4% respectively), and `set_allocation`'s transaction is
16% of the size limit. **No cap change is proposed**: both maxima are
deliverable, with more than 2× headroom on every resource measured.

The test asserts every row at **50% of the limit**, not at the limit itself, so
a future field does not merely avoid aborting today — it has to fit the same
headroom the caps were accepted with.

### What these numbers are, and are not

- **Instructions and memory are host-model figures for native Rust execution**
  under the test host. The SDK notes they understate the WASM-equivalent cost
  (no VM instantiation, no Wasm reads/rent). A 0.9%-of-limit CPU figure is not
  a promise that a deployed WASM call costs the same; it is a demonstration
  that the *input size* the caps allow is nowhere near the instruction budget.
  Absolute testnet fees stay in the sessions above.
- **Read bytes need a stated substitution.** The test host keeps live Soroban
  state in memory, and its own `InvocationResources::disk_read_bytes` doc says
  disk reads are "most of the time 0 or almost 0" there. The suite therefore
  uses the serialized instance entry — the entry a real host would have to
  restore — as the restore-size floor. That is conservative for these calls,
  whose only large read is that entry, and it is labelled as such in the test.
  Read **entries** are the host's in-memory count, which is an upper bound on
  the disk-read entries the network limit applies to.
- **Transaction size is modelled, not simulated.** The host's
  `CostEstimate::resources()` explicitly does not model transaction size. The
  suite measures the XDR size of the `InvokeContractArgs` the transaction must
  carry (the 200-member list dominates at 20,072 bytes) and adds a fixed 1 KiB
  allowance for the envelope, footprint keys and one signature. The ledger
  entry's *contents* are not part of the transaction envelope; they are
  measured separately in the ledger-entry row.

### Reproduce

```bash
cd contracts/escrow
cargo test --test resource_limits -- --nocapture
```

The suite prints the full used/limit/margin table for each stage and fails if
any row exceeds the stated 50% bound. It also asserts that the `settle` fixture
really does hold both maxima in one instance entry, so a future storage-layout
change that moved `Allocation` to its own key would fail loudly instead of
making the `settle` figure look better than the commitment it claims to model.

---

## Oracle group — `oracle_rate`, `resolve_fx_shortfall` against the real Reflector — `tests/reflector_oracle_costs.rs`

Every other oracle measurement in this repo, including the `budget.rs` rows
above, uses a hand-written mock whose `lastprice` is one instance-storage
read. This group runs the FX path against the **deployed testnet Reflector
"Fiat exchange rates" binary** and compares the two.

**The binary.** `contracts/escrow/tests/wasm/reflector_fiat_testnet.wasm`,
fetched from `CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W` on
2026-11-25, sha256
`25fc306065cbb9b05461db78ec259802cb916053ba5b3ab911b0a72dd1a81439` (20,417
bytes). The test imports it with `contractimport!(…, sha256 = …)`, so a
changed file is a build error. Provenance and the re-fetch procedure are in
[`tests/wasm/README.md`](../contracts/escrow/tests/wasm/README.md).

**Configured like testnet, through Reflector's own entry points.** The
test registers the WASM and calls its admin `config` with the values read
from the live instance's storage the same day: base `USD`, 14 decimals, 9
assets (EUR, GBP, CHF, CAD, MXN, ARS, BRL, THB, XAU), 5-minute resolution,
1-day history retention, cache size 2, and a fee config. It then publishes
an hour of rounds (12) with admin `set_price`. Two shapes:

- **live**: single-asset EUR rounds, with the escrow quoting EUR. This is
  exactly what testnet publishes today; the other eight feeds have expired.
- **full round**: all 9 assets in every round, with the escrow quoting XAU
  (the last index). This gives the largest cached round and the longest
  mask walk, so it is the most expensive successful read.

### Results (host model, `env.cost_estimate()`, measured 2026-11-25)

| Call | Oracle | CPU insns | Mem bytes | Read entries | Write entries | Write bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `oracle_rate` | MockOracle (native) | 237,472 | 121,194 | 4 | 0 | 0 |
| `oracle_rate` | Reflector, live | **843,484** | 398,917 | 5 | 0 | 0 |
| `oracle_rate` | Reflector, full round | **872,012** | 413,157 | 5 | 0 | 0 |
| `resolve_fx_shortfall` | MockOracle (native) | 472,322 | 201,858 | 6 | 1 | 1,744 |
| `resolve_fx_shortfall` | Reflector, live | **1,532,007** | 626,974 | 7 | 1 | 1,744 |
| `resolve_fx_shortfall` | Reflector, full round | **1,560,951** | 642,878 | 7 | 1 | 1,744 |
| `Reflector.lastprice` (direct) | live | 626,213 | 350,854 | 2 | 0 | 0 |
| `Reflector.lastprice` (direct) | full round | 654,741 | 365,094 | 2 | 0 | 0 |
| `Reflector.decimals` (direct) | live | 535,586 | 349,430 | 2 | 0 | 0 |

### Reading it

- **The real oracle makes the FX path ~3.2–3.7× as expensive in CPU as the
  mock**: `oracle_rate` gains about 606k instructions and
  `resolve_fx_shortfall` about 1.06M. Memory grows by about the same factor.
  Each call also reads one more entry (Reflector's contract code). Write
  figures don't change, because the oracle writes nothing.
- **Almost all of the gap is running a WASM contract at all, not
  Reflector's price lookup.** A bare `decimals()` call, which only loads the
  instance and returns a constant, already costs 536k. `lastprice` adds just
  ~91k on top (~119k for the full round). The escrow runs as native Rust in
  this harness and the mock is native too, so the mock rows contain no VM
  cost at all.
- **`resolve_fx_shortfall` pays that VM cost twice.** Its extra ~1.06M is
  roughly one `lastprice` (~606k) plus one `decimals` (~454k), because each
  cross-contract call instantiates Reflector again. Removing the `decimals()`
  call (e.g. by pinning the oracle's decimals in `OracleConfig`) would save
  about 30% of the call's CPU. That would conflict with the decision recorded
  in [`reflector-decimals-assumption.md`](./reflector-decimals-assumption.md),
  so it is noted here, not proposed.
- **The full-round worst case adds only ~2–3%.** Reflector's per-asset work
  is small next to its fixed cost.

### Against the per-transaction limits: not close

Measured against the same testnet limits `tests/resource_limits.rs` uses,
the heaviest row (`resolve_fx_shortfall`, full round) is at **0.39% of CPU**
(1.56M / 400M), **1.5% of memory** (0.64 MB / 40 MiB), **3.5% of read
entries** (7 / 200) and **1.3% of write bytes**. The test asserts every row
stays under 50% of each limit. **No flag:** the FX path is a few times more
expensive than the mock suggested, but it is nowhere near a limit.

### A behaviour the mock does not model (pinned by `lastprice_sees_only_the_latest_round`)

This binary's `lastprice(asset)` only looks at the **latest round**. If that
round does not quote the asset, it returns `None`, even though a round
5 minutes earlier still holds a valid price (`price(asset, t)` returns it).
The public `reflector-contract` source later added a fallback to the history
bitmask (commit `fa1b9ac`); the deployed binary behaves as if it predates
that. For the escrow, one round that skips the configured asset makes
`oracle_rate` and `resolve_fx_shortfall` fail with
`OraclePriceUnavailable` until the next round quotes it. That is a liveness
gap, not a safety one: nothing is written, and a retry succeeds. It also
means a cache-miss (temporary-storage) read can't be reached through
`lastprice` with a cache size of 2, so there is no such scenario to measure.

### Caveats

- These are **host-model** figures, the same kind as `budget.rs`, not
  testnet fees. The escrow runs as native Rust, so its own share is
  understated. In production it would be WASM as well, and every call it
  makes would also carry its own VM cost. The opt-in
  `fx_path_cost_with_escrow_as_wasm` test measures that shape when given a
  release build (below); its figures are not recorded here yet.
- The mock rows differ slightly from `budget.snapshot` (237,472 vs 236,796
  for `oracle_rate`) because this suite uses different terms and a real
  quoted asset; the mock is otherwise the same shape.

### Reproduce

```sh
cargo test -p harvestlock-escrow --test reflector_oracle_costs -- --nocapture

# Opt-in: the escrow as WASM too (needs stellar-cli for the build).
stellar contract build --locked
ESCROW_WASM=$PWD/target/wasm32v1-none/release/harvestlock_escrow.wasm \
  cargo test -p harvestlock-escrow --test reflector_oracle_costs \
  fx_path_cost_with_escrow_as_wasm -- --nocapture
```

---

## Results — delivery group (`mark_checkpoint`, `confirm_delivery`)

Session 2, measured against instance
`CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ`
([stellar.expert](https://stellar.expert/explorer/testnet/contract/CCTQTECYATFFE3GJWYSND6CJOGB6KLGSF3JKK5KW3HRBNTKLFBFJEUWJ)),
ledgers ~5290578–5290585 (`mark_checkpoint`) and ~5290754–5290757
(`confirm_delivery`).

One row per method. "Fee" columns are stroops (1 XLM = 10,000,000 stroops);
`Fee (total)` is the estimator's non-refundable + refundable sum — read the
caveats section before treating it as "what a transaction costs".

| Method | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) | Fee (total XLM) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `mark_checkpoint` | 1,523,347 | 0 | 2 | 2 | 144 | 1,584 | 148 | 4,579 | 901,791 | 906,370 | 0.0906370 |
| `confirm_delivery` | 1,530,217 | 0 | 2 | 2 | 144 | 1,580 | 164 | 4,584 | 902,392 | 906,976 | 0.0906976 |

`confirm_delivery` was measured with `delivered_quantity=1000`,
`grade_index=0` — full delivery at the top grade, against a commitment
initialized with `contracted_quantity: 1000` and
`grade_price_bps: [10000, 9000, 7500]`. `settlement_bps` came back as
`10000` on a fresh `get_commitment` read after the call, so the arguments
really did drive the contract's arithmetic path, not a rejected call.

**Reproducibility:** `confirm_delivery` was re-measured on a second,
independent instance
(`CDKP2KUIDUJ4MS3G4GDS372BK3HMRZZEMJGM4C5GULTGAISI2FFGZAXK`, ledger
5291028) and returned **identical** resource figures — 1,530,217 CPU, 2 read
/ 2 write entries, 144 / 1,580 bytes, tx size 164, 4,584 non-refundable
stroops. Only the refundable rent half moved (902,392 → 903,157).

## Same-session controls

Measured on instance
`CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV`
([stellar.expert](https://stellar.expert/explorer/testnet/contract/CBTFFKL24HYNUUG4K26FBB3B47T5KROWR3FTMBH4SST5NA3NCT5IFATV)),
ledgers ~5290798–5290808, **same WASM, same SDK, same day** as the two rows
above. These exist so the delivery-group numbers are readable against a
baseline rather than against a table measured on a different build.

| Method | Auth | Moves money? | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `release_advance_1` | none | no | 996,645 | 0 | 1 | 1 | 0 | 1,508 | 152 | 2,359 | 194,633 | 196,992 |
| `mark_checkpoint` | warehouse | no | 1,523,347 | 0 | 2 | 2 | 144 | 1,584 | 148 | 4,579 | 902,337 | 906,916 |
| `claim_advance_1` | cooperative | yes (15,000,000) | 1,671,867 | 0 | 2 | 4 | 144 | 1,952 | 148 | 7,123 | 902,922 | 910,045 |

Two things fall out of this table, and both are reproducible:

- **`mark_checkpoint` measured on three independent instances**
  (`CCTQTE…`, `CDG2V5…` and the control instance `CBTFFK…`) **returned
  byte-identical resource figures** — 1,523,347 CPU, 2 read / 2 write
  entries, 144 / 1,584 bytes, 4,579 non-refundable stroops. Only the
  refundable (rent) half moved, by ~550 stroops, because the simulated TTL
  horizon differed by a couple of ledgers.
- **The auth-gated call costs ~53% more CPU than the permissionless one it
  sits next to**, and buys one extra read entry + one extra write entry.
  Everything else is held constant: same instance, same build, same
  contract balance, same shape of `Commitment` written back. The only
  differences between the `release_advance_1` row and the `mark_checkpoint`
  row are the `require_auth()` call and the status value being written —
  and a status discriminant cannot account for 527k CPU instructions or a
  whole extra ledger entry. The extra read+write entry appears only on the
  auth-gated path.

## ⚠️ Flag — this group moves zero tokens, yet costs ~53% more than the permissionless equivalent, because `require_auth()` adds a ledger entry

`mark_checkpoint` and `confirm_delivery` are pure bookkeeping — `mark_checkpoint`
flips one status, `confirm_delivery` records a quantity, a grade and a
`settlement_bps` — and **neither moves a single stroop**. Even so, at
1.52M–1.53M CPU instructions they land within 9% of `claim_advance_1`
(1.67M), which *does* move 15,000,000 stroops to the cooperative. The gap
is entirely the auth-gated path: `require_auth()` on the warehouse
operator's address adds one read entry, one write entry and ~527k CPU
instructions (~53% over `release_advance_1`'s 996,645) on top of a state
transition that is otherwise identical in shape. The auth is load-bearing
here — `mark_checkpoint` *is* the warehouse operator's attestation, and
`confirm_delivery`'s grade/quantity call is authoritative per PRD §7 — so
this is a cost to budget for, not a saving to bank.

## ⚠️ Flag — 99.5% of each `Fee (total)` is simulated storage rent, not execution

The estimator's `Fee (total)` for this group (~906,000 stroops / ~0.0906
XLM) is **not** what a transaction costs: 901,791 of the 906,370 is the
*refundable* rent half, which is an artifact of the simulation extending
entry TTLs to the simulation horizon. The **real** submitted transactions
were charged **4,181 stroops** (`mark_checkpoint`) and **4,242 stroops**
(`confirm_delivery`) in total — see the CLI cross-check below. For
budgeting, use the non-refundable column (4,579 / 4,584 stroops) plus the
CLI's real-submission numbers, not the total.

---

## Results — advance-tranche group (session 1, retained)

Real resource-cost numbers for the six advance-tranche methods
(`release_advance_1`/`_2`, `claim_advance_1`/`_2`, `reclaim_advance_1`/`_2`),
measured with the same tool against freshly-deployed testnet instances of
this contract.

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

| Method | CPU instructions | Memory bytes | Read entries | Write entries | Read bytes | Write bytes | Tx size | Fee non-refundable | Fee refundable | Fee (total stroops) | Fee (total XLM) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `release_advance_1` | 936,397 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,341 | 13,233 | 15,574 | 0.0015574 |
| `release_advance_2` | 939,162 | 0 | 1 | 1 | 0 | 1,476 | 152 | 2,342 | 6,456 | 8,798 | 0.0008798 |
| `claim_advance_1` | 1,611,598 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `claim_advance_2` | 1,611,619 | 0 | 2 | 4 | 144 | 1,920 | 148 | 7,105 | 839,761 | 846,866 | 0.0846866 |
| `reclaim_advance_1` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |
| `reclaim_advance_2` | 1,609,708 | 0 | 2 | 4 | 144 | 1,920 | 152 | 7,105 | 839,778 | 846,883 | 0.0846883 |

### ⚠️ Flag — claim/reclaim fees look ~54–96× a release, but 99.2% of that is simulated storage rent, not execution

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

---

## Reading the numbers

- **Memory bytes = 0 on every row, in both sessions** — the tool found no
  memory-bytes figure in the current-protocol `simulateTransaction` response
  to decode; use CPU instructions as the compute metric. It is *not* a claim
  that these calls use no memory.
- **The auth-gated calls are the expensive ones**, not the ones that move
  money. `mark_checkpoint`/`confirm_delivery` (no money) cost ~53% more CPU
  than `release_advance_1` (no money, no auth), and ~91% of what
  `claim_advance_1` costs while moving 15,000,000 stroops. The cost is the
  authorization entry, not the arithmetic and not the transfer.
- **`release_advance_1`/`release_advance_2` are the cheapest calls in the
  contract** (~996k CPU session 2, ~936–939k session 1) — they only move a
  deadline in instance storage. The whole 15,574-vs-8,798 stroop gap between
  the two in session 1 sits in the *refundable* half (13,233 vs 6,456); the
  execution halves differ by 1 stroop. That's a fresh-instance TTL artifact of
  when the simulation ran, not a difference in contract logic — the two
  functions share one private `open_tranche` implementation by design (see
  HANDOFF's design decisions).
- **The claims and reclaims are all the same cost within noise** (~1.610–1.612M
  CPU session 1, 1.672M session 2): tranche 1 and 2 route through the same
  private `claim_tranche`/`reclaim_tranche`, and claim-vs-reclaim only flips a
  deadline comparison. The extra CPU over a release is the SEP-41 SAC
  `transfer` cross-contract call (and its events) that releases don't make,
  plus the auth entry both of them carry.
- **The two sessions' absolute numbers are not directly comparable** — SDK
  28.0.0 → 29.0.1 and a 52,119 → 60,043-byte WASM. `claim_advance_1` moved
  1,611,598 → 1,671,867 CPU (+3.7%) between them; the *deltas* between
  methods moved almost not at all (session 1's claim-minus-release delta was
  675,201 CPU; session 2's was 675,222). That stability is the reason the
  same-session control table above is worth more than the absolute numbers.
- **Session 2's WASM is 60,043 bytes** vs session 1's 52,119 — the delta is
  the SDK bump plus the `status_changed` event surface, not a change to the
  methods measured here.

---

## Cross-check against the CLI (the source of record, per `cost-estimation.md`)

Both methods in this group were **actually submitted** (not just simulated)
with `stellar contract invoke … --cost`, as an independent check on the
estimator's fee math.

### `mark_checkpoint`

Real submission:
tx [`6c30cd83…`](https://stellar.expert/explorer/testnet/tx/6c30cd83099a717733cac0e5ac31a80e783893f4bc1626270a1e3b01546b1657)
(ledger 5290588), instance `CCTQTE…`.

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 4,579 | 3,680 charged |
| Refundable / rent | 901,791 proposed-in-footprint | 5,300 proposed → 401 charged, 4,899 refunded |
| Total resource fee | 906,370 | 4,081 charged |
| Inclusion fee | — | 100 charged |
| **Total charged** | — | **4,181** |

### `confirm_delivery`

Real submission:
tx [`a42c212a…`](https://stellar.expert/explorer/testnet/tx/a42c212a5a124c9adcc55c8c944a5d6d1da4959373db52fd04f07b10b58d3ec6)
(ledger 5290760), instance `CCTQTE…`, `delivered_quantity=1000`,
`grade_index=0`.

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 4,584 | 3,751 charged |
| Refundable / rent | 902,392 proposed-in-footprint | 5,288 proposed → 391 charged, 4,897 refunded |
| Total resource fee | 906,976 | 4,142 charged |
| Inclusion fee | — | 100 charged |
| **Total charged** | — | **4,242** |

The ~830–900-stroop non-refundable difference is envelope shape (the CLI's real
transaction carries auth/footprint details the estimator's synthetic envelope
doesn't); the rent gap is the story flagged above. Note that
`stellar contract invoke --send=no --cost` prints **no** cost block on
stellar-cli v28 — cost only prints on a submitted transaction.

For completeness, session 1's cross-check on `reclaim_advance_2` was:

| | Estimator (simulation) | CLI (real submission) |
| --- | ---: | ---: |
| Non-refundable (execution) | 7,105 | 7,620 charged |
| Refundable / rent | 839,778 proposed-in-footprint | 5,377 proposed → 469 charged, 4,908 refunded |
| Total resource fee | 846,883 | 8,089 charged |
| Inclusion fee | — | 100 charged |

---

## Tooling finding — `soroban-cost-estimator` v0.1.0 cannot encode a `u32` argument

This is the one thing in this session that needed working around, and it is
an upstream limitation, not a local misconfiguration — worth reporting to the
upstream project.

`confirm_delivery` takes `delivered_quantity: u32` and `grade_index: u32`. The
estimator's `--arg` infers the argument's type **from the literal alone** —
`parse_arg_scval` in `src/xdr_helper.rs` returns `ScVal::I64` for any integer
that fits in an `i64` — and ignores the contract spec the tool has already
parsed. The host then rejects the call before the contract body runs:

```text
$ soroban-cost-estimator estimate --wasm … --id CCTQTE… \
    --fn confirm_delivery --arg delivered_quantity=1000 --arg grade_index=0 --network testnet
Error: Simulation failed: HostError: Error(WasmVm, InvalidAction)

Event log (newest first):
   0: [Diagnostic Event] contract:CCTQTE…, topics:[error, Error(WasmVm, InvalidAction)],
      data:["VM call trapped: UnreachableCodeReached", confirm_delivery]
   1: [Diagnostic Event] topics:[fn_call, CCTQTE…, confirm_delivery], data:[1000, 0]
```

The SDK's macro-generated wrapper converts each incoming argument with
`try_from_val_for_contract_fn(...).unwrap_optimized(...)` (see
`soroban-sdk-macros`' `derive_fn.rs`), and a `u32` parameter's `ScVal` is the
`U32` tag of Soroban's tagged union — an `ScVal::I64` is not one, so the
conversion fails, `unwrap_optimized` panics, and the contract crate's
`panic = "abort"` release profile turns that into the
`UnreachableCodeReached` the host reports. `stellar-cli` does not hit this
because it encodes arguments from the contract spec.

**This is not fixed on upstream `main` either.** `main` (commit `9627919`)
adds `validate_args_against_spec`, which *accepts* `1000` for a `u32`
parameter — and then still encodes the value as `ScVal::I64`, so it fails the
same way, only later. The upstream README's claim that "the inferred type
drives the simulation's serialization size, which is what the fee math
actually depends on" is true for the fee math, but the inference also has to
survive the contract's own argument decoding, and it doesn't.

The limitation is wider than `u32`. The estimator's `--arg` can express
`bool`, integers and strings, and nothing else — so it cannot invoke a
function that takes an `Address`, a `Vec<_>` or an `Option<_>` either.
`initialize` takes all three, which means the estimator cannot measure the
*instantiation* of a new commitment at all (see
[WASM Upload vs Instantiation](#wasm-upload-vs-instantiation) below). That
is a real gap in the tooling, and worth reporting upstream alongside the
`u32` one. It does not affect the measurements in this document: what is
being measured is an invocation **against an instance that is already
deployed and already initialized**.

### The patch used here

A two-hunk patch to the v0.1.0 source, applied on top of the upstream
`v0.1.0` tag (`6903d3e`), built with `cargo build --release --locked`:

```diff
--- a/src/main.rs
+++ b/src/main.rs
@@ -214,7 +214,24 @@
 
     let sc_vals: Vec<stellar_xdr::ScVal> = args
         .iter()
-        .map(|a| xdr_helper::parse_arg_scval(a))
+        .enumerate()
+        .map(|(i, a)| {
+            // Resolve the declared parameter type from the contract spec so
+            // the value is encoded the way a spec-aware client would encode
+            // it (an `ScVal::U32` for a `u32` parameter, not an `ScVal::I64`).
+            // The `--arg` key is matched against the parameter name first,
+            // falling back to positional order.
+            let key = a.split_once('=').map(|(k, _)| k).unwrap_or("");
+            let param = fn_name
+                .and_then(|name| wasm_info.functions.iter().find(|f| f.name == name))
+                .and_then(|f| {
+                    f.params
+                        .iter()
+                        .find(|p| p.name == key)
+                        .or_else(|| f.params.get(i))
+                });
+            xdr_helper::parse_arg_scval_typed(a, param.map(|p| p.type_name.as_str()))
+        })
         .collect();
```

```diff
--- a/src/xdr_helper.rs
+++ b/src/xdr_helper.rs
@@ -273,6 +273,64 @@
      stellar_xdr::ScVal::String(sc_string)
  }
+
+/// Parse a single `--arg` value into an `ScVal`, honouring the contract
+/// spec's declared parameter type when one is known.
+///
+/// Upstream v0.1.0 always infers the type from the literal (`i64` for any
+/// integer that fits), which traps inside the contract for a parameter the
+/// spec declares as `u32`/`i32`/…: the SDK's macro-generated wrapper
+/// converts the incoming `ScVal` with
+/// `try_from_val_for_contract_fn(...).unwrap_optimized(...)`, and a `u32`
+/// parameter's `ScVal` is the `U32` tag — an `ScVal::I64` is not one, so the
+/// conversion fails and the VM aborts with `UnreachableCodeReached` before
+/// the contract body runs.
+///
+/// This variant encodes the value with the declared type when the spec's type
+/// name is one of the modelled primitives, and otherwise falls back to the
+/// upstream inference. The measurement path (RPC simulation, footprint decode,
+/// fee math) is untouched — only the argument's `ScVal` encoding changes, and
+/// it changes to exactly what a spec-aware client such as `stellar-cli`
+/// sends.
+pub fn parse_arg_scval_typed(arg: &str, type_name: Option<&str>) -> stellar_xdr::ScVal {
+    let value = arg.split_once('=').map(|(_, v)| v).unwrap_or(arg);
+
+    match type_name {
+        Some("bool") => {
+            if let Ok(b) = value.parse::<bool>() {
+                return stellar_xdr::ScVal::Bool(b);
+            }
+        }
+        Some("u32") => {
+            if let Ok(n) = value.parse::<u32>() {
+                return stellar_xdr::ScVal::U32(n);
+            }
+        }
+        Some("i32") => {
+            if let Ok(n) = value.parse::<i32>() {
+                return stellar_xdr::ScVal::I32(n);
+            }
+        }
+        Some("u64") => {
+            if let Ok(n) = value.parse::<u64>() {
+                return stellar_xdr::ScVal::U64(n);
+            }
+        }
+        Some("i64") => {
+            if let Ok(n) = value.parse::<i64>() {
+                return stellar_xdr::ScVal::I64(n);
+            }
+        }
+        Some("u128") | Some("i128") => {
+            // `stellar-xdr` 27 models 128-bit ints as `Int128Parts`, not a
+            // bare `i128`/`u128`; leave those to the upstream inference
+            // rather than hand-rolling the parts encoding here.
+        }
+        _ => {}
+    }
+
+    parse_arg_scval(arg)
+}
 
 #[cfg(test)]
 mod tests {
     use super::*;
```

### Why the patch doesn't change any measured value

The patch touches **only** how an `--arg` literal becomes an `ScVal`. The
measurement path — RPC `simulateTransaction`, footprint decode, fee math — is
untouched. That was verified rather than asserted: the stock v0.1.0 binary and
the patched binary were both run against `mark_checkpoint` (a zero-argument
function, so the patched code path is a no-op for it) on the same instance and
produced **byte-identical JSON**:

```json
{"function":"mark_checkpoint","wasm_hash":"e097f1ff…","cpu_instructions":1523347,
 "memory_bytes":0,"tx_size":148,"read_entries":2,"write_entries":2,
 "read_bytes":144,"write_bytes":1584,
 "fee":{"non_refundable_stroops":4579,"refundable_stroops":902337,
        "total_stroops":906916,"total_xlm":"0.0906916"}}
```

Every zero-argument row in this document was produced by the **stock,
unpatched** v0.1.0 binary. `confirm_delivery` is the only row that required
the patch, and only because of its `u32` parameters.

---

## Reproduce

### Session 2 — delivery group

```bash
# 0. Toolchain: Rust stable + the wasm32v1-none target, stellar-cli v28.1.0,
#    and soroban-cost-estimator v0.1.0 (crates.io).
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Build + upload the WASM (from repo root)
stellar contract build --locked
WASM_HASH=$(stellar contract install \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet)
# → e097f1ffe08f878007cd9511d5fc70da0e238ac64491181e158d58f91e4714c5

# 2. Deploy two instances: A for the walk-through, C for the controls.
A=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)
C=$(stellar contract deploy --wasm-hash "$WASM_HASH" --source-account deployer --network testnet)

# 3. Initialize both (identical parameters), then lock.
#    native XLM SAC, 100,000,000 stroops, 15%/20% advance split.
#    Note: For issued assets (e.g. HLUSD/USDC), see scripts/test-asset.sh
#    and docs/trustline-prerequisites.md for trustline prerequisites.
for id in "$A" "$C"; do
  stellar contract invoke --id "$id" --source-account buyer --network testnet -- \
    initialize \
    --buyer "$BUYER" --cooperative "$COOP" --warehouse-operator "$WAREHOUSE" \
    --token CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
    --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000 \
    --claim-window-secs 3600 --remainder-window-secs 3600 --delivery-window-secs 86400 \
    --contracted-quantity 1000 --grade-price-bps '[10000,9000,7500]' --oracle-config null
  stellar contract invoke --id "$id" --source-account buyer --network testnet -- lock
done

# 4. Control row: release_advance_1 on C, while C is Locked.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$C" --fn release_advance_1 --network testnet

# 5. mark_checkpoint: estimate FIRST (a simulation does not advance state),
#    then submit for the CLI cross-check.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn mark_checkpoint --network testnet
stellar contract invoke --id "$A" --source-account warehouse --network testnet --cost -- \
  mark_checkpoint

# 6. Walk A to ReadyForDelivery: release tranche 2, claim both tranches,
#    open the remainder window, fund the remainder.
stellar contract invoke --id "$A" --source-account deployer     --network testnet -- release_advance_2
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- claim_advance_1
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- claim_advance_2
stellar contract invoke --id "$A" --source-account cooperative --network testnet -- ready_for_delivery
stellar contract invoke --id "$A" --source-account buyer        --network testnet -- fund_remainder

# 7. confirm_delivery: estimate (needs the spec-aware `--arg` patch — see above),
#    then submit for the CLI cross-check.
soroban-cost-estimator estimate \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --id "$A" --fn confirm_delivery \
  --arg delivered_quantity=1000 --arg grade_index=0 --network testnet
stellar contract invoke --id "$A" --source-account warehouse --network testnet --cost -- \
  confirm_delivery --delivered_quantity 1000 --grade_index 0

# 8. Prove the patch didn't move any number: run the STOCK binary and the
#    PATCHED binary against the same zero-argument call and diff the JSON.
#    (Build the patched binary from the upstream v0.1.0 tag with the two-hunk
#    patch above; `cargo build --release --locked`.)
soroban-cost-estimator estimate --wasm … --id "$C" --fn mark_checkpoint --network testnet --json
./soroban-cost-estimator-patched estimate --wasm … --id "$C" --fn mark_checkpoint --network testnet --json
# → byte-identical output

# 9. Independent reproduction: deploy a second instance, walk it to
#    ReadyForDelivery the same way, and re-measure confirm_delivery. The
#    resource figures come back identical; only the rent half moves.
```

### Session 1 — advance-tranche group

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
- **Every instance here is a validation artifact**, per HANDOFF's standing
  rule — and they've now been walked past these states (A's delivery was
  genuinely confirmed; B, C and D are controls and reproductions).
  Re-running the estimates against *these* IDs will hit state guards and
  error; deploy fresh per the reproduce steps.
- **`confirm_delivery` needs the spec-aware `--arg` patch** — see
  [Tooling finding](#tooling-finding--soroban-cost-estimator-v010-cannot-encode-a-u32-argument).
  Everything else in this document runs on the stock v0.1.0 binary.
- **The two sessions are not directly comparable** — different SDK, different
  WASM. Use the same-session control table for deltas.
- **The upload/deploy/`initialize` rows were measured on a different artifact
  from the per-method rows above**, and that is unavoidable: this
  environment's `stellar-cli` was built without the optional `additional-libs`
  feature, so `stellar contract optimize` (and `build --optimize`) is a no-op
  and the local build is the unoptimised **82,567-byte** module rather than
  the ~15 KB optimized artifact the earlier sessions measured. The
  upload/rent rows scale with module size (hence the large upload figure); the
  `initialize` rows' non-refundable half (4,893–5,398 stroops) is
  execution-only and does not. Re-measure on an optimized build before
  comparing those absolute numbers against the per-method tables.
- The SAC balance entries these claims/reclaims touched were created moments
  earlier by `lock` on the same instances. A nth-claim in a long-lived
  commitment may differ in the rent half; the execution half is state-size
  independent here.
- **Session 2's instances were funded from the testnet friendbot** and their
  keys are throwaways, regenerated for this session. Do not reuse them for
  anything.

## WASM Upload vs Instantiation

The deploy flow separates three distinct costs: uploading a WASM version
(once per version), creating a contract instance from that version (once
per commitment), and `initialize`-ing that instance (once per commitment).

**Measured 23 Nov 2026 against testnet**, with the `stellar-cli` recorded in
[`cost-estimation.md`](./cost-estimation.md) §5 (**30.0.1**, `stellar-xdr`
30.0.0), for the artifact this repo builds locally:

- `target/wasm32v1-none/release/harvestlock_escrow.wasm`, **82,567 bytes**
- SHA-256 **`70a9ef3240a0f198f94df687aad215e454dca2f7fa49920af8288767eb112ab6`**

Every figure below is a **real charge from a submitted testnet transaction**
— not a simulation estimate. `stellar contract upload|deploy|invoke --cost`
prints the fee box after submission, and its rows are what these columns
are: *inclusion* fee, the **non-refundable** resource fee (CPU
instructions + storage read/write + transaction size), the **rent** charged
up front (storage rent, plus the return value and events that ride in the
same refundable bucket), how much of that refundable part the network
returned once actual usage was known, and the net cost that actually left
the account.

### Upload, deploy, and `initialize`

| Operation | Variant | Inclusion | Non-refundable resource fee | Rent charged | Rent refunded | **Net cost (stroops)** | Notes |
|-----------|---------|-----------|-----------------------------|--------------|---------------|------------------------|-------|
| **WASM upload** | 82,567-byte module, first upload of this hash | 100 | 238,760 | 30,854,171 | 4,650,296 | **26,442,735** | One-time per contract version; hash pinned in deployment config |
| **Instantiation** | `stellar contract deploy --wasm-hash` | 100 | 3,342 | 12,188 | 6,667 | **8,963** | Marginal cost per commitment for the instance entry alone |
| **`initialize`** | 3-grade schedule, no `oracle_config` | 100 | 4,893 | 4,092,383 | 618,696 | **3,478,680** | The ordinary pilot shape |
| **`initialize`** | 16-grade schedule, no `oracle_config` | 100 | 5,398 | 4,366,765 | 659,853 | **3,712,410** | `MAX_GRADE_ENTRIES` costs ~233,730 more |
| **`initialize`** | 3-grade schedule **+** `oracle_config` | 100 | 4,893 | 4,092,387 | 618,696 | **3,478,684** | The oracle config adds ~4 stroops of rent |
| **`initialize`** | 16-grade schedule **+** `oracle_config` | 100 | 5,398 | 4,370,843 | 660,465 | **3,715,876** | Worst-case onboarding shape |

**The per-commitment onboarding number the API needs** is *deploy +
initialize*, paid every time a commitment is created:

| Onboarding shape | Deploy | `initialize` | **Total (stroops)** | Total (XLM) |
|------------------|--------|--------------|---------------------|-------------|
| 3-grade, no oracle (default) | 8,963 | 3,478,680 | **3,487,643** | 0.3487543 |
| 16-grade + oracle (worst case) | 8,963 | 3,715,876 | **3,724,839** | 0.3724839 |

So onboarding a commitment costs about **0.35 XLM**, and the grade-count and
oracle-config knobs move that by at most ~0.024 XLM. Uploading the version
itself (~2.64 XLM) is a one-time-per-version cost, not a per-commitment one.

Two findings worth carrying into the API's fee budget rather than reading
off the table alone:

- **`initialize`'s cost is dominated by rent, not computation.** 4,092,383 of
the 4,097,376 charged stroops (99.9%) is the rent bucket, because the first
call on any instance runs `bump_instance_ttl`, which extends the instance
entry to the network maximum (see `docs/storage-ttl.md`). The non-refundable
execution half is **4,893 stroops** — 0.00049 XLM — for every variant. A
future change that re-rents less aggressively would move this row more than
any code optimisation could.
- **The first `initialize` after an upload is far more expensive than the
rest.** Measured on a fresh deployment of a freshly uploaded hash:
766,642,875 stroops charged, 115,000,228 refunded → **651,642,647 net**
(~65 XLM), about 187× the steady-state `initialize` above. That is
consistent with `Instance::extend_ttl` extending the *shared WASM code
entry* along with the instance: rent on the code entry scales with the
82,567-byte module, so whoever calls first on that hash pays it, once per
uploaded hash rather than once per commitment. It does not change the
per-commitment budget above (by then the code entry's TTL is already at
max), but it is a real deployment-day cost and it is why a single
`initialize` measured in isolation can look alarming.

### TTL extension rent tradeoff

The fresh-instance `initialize` row above is the available measured
max-TTL-extension baseline: **4,092,383 stroops gross rent** (618,696 refunded
in that simulation). To put a deadline-sized extension in context, assuming
linear rent, a five-second ledger, and the test host's 6,312,000-ledger maximum:

| Extension target | TTL ledgers | Gross rent |
| --- | ---: | ---: |
| Max TTL (measured `initialize`) | 6,312,000 | 4,092,383 |
| 120 days (pro-rata estimate) | 2,073,600 | ~1,344,000 |
| Difference | 4,238,400 | ~2,748,000 |

The 120-day row is a pro-rata estimate from the measured baseline, **not** a
second transaction measurement. It excludes any safety margin and later
remainder, FX, or dispute windows; network rent settings, ledger interval,
entry size, and current TTL also affect the actual fee. The contract retains
max-TTL because `Draft` can be locked later and successfully resolved disputes
can be reopened without consuming the lapse cap, so the current state machine
does not provide a finite latest reachable deadline to use as a safe cap. See
[`storage-ttl.md`](./storage-ttl.md#ttl-target-decision) for the reasoning.

### Reproduce

```bash
# 0. The exact artifact measured (from repo root). Note the byte size and
#    hash — this environment's stellar-cli was built without the optional
#    `additional-libs` feature, so `--optimize` / `stellar contract optimize`
#    is a no-op here and the artifact is the unoptimised 82,567-byte module.
stellar contract build --locked
sha256sum target/wasm32v1-none/release/harvestlock_escrow.wasm
# 70a9ef3240a0f198f94df687aad215e454dca2f7fa49920af8288767eb112ab6

# 1. A funded throwaway identity (testnet friendbot).
stellar keys generate deployer --network testnet --fund

# 2. Upload — once per WASM version. --cost prints the fee box after
#    submission; the hash goes to stdout.
WASM_HASH=$(stellar contract upload \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet --cost 2>/dev/null | tail -1)

# 3. Deploy — once per commitment, from the already-uploaded hash.
ID=$(stellar contract deploy --wasm-hash "$WASM_HASH" \
  --source-account deployer --network testnet --cost 2>/dev/null | tail -1)

# 4. Initialize — once per instance, on a FRESH deploy for each variant below
#    (initialize is one-shot; a second call is AlreadyInitialized).
#    The native XLM SAC is used as `token`; the buyer is the source account.
#    (For issued classic assets and trustline setup, see scripts/test-asset.sh & docs/trustline-prerequisites.md).
TOKEN=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
BT="--buyer $(stellar keys address deployer) --cooperative $(stellar keys address coop)"
BT="$BT --warehouse-operator $(stellar keys address wh) --token $TOKEN"
BT="$BT --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000"
BT="$BT --claim-window-secs 3600 --remainder-window-secs 3600"
BT="$BT --delivery-window-secs 86400 --contracted-quantity 1000"

# 4a. 3 grades, no oracle
stellar contract invoke --id "$ID" --source-account deployer --network testnet --cost -- \
  initialize $BT --grade-price-bps '[10000,9000,7500]' --oracle-config null

# 4b. 16 grades, no oracle (MAX_GRADE_ENTRIES)
stellar contract invoke --id "$ID" --source-account deployer --network testnet --cost -- \
  initialize $BT --oracle-config null \
  --grade-price-bps '[10000,9800,9600,9400,9200,9000,8800,8600,8400,8200,8000,7800,7600,7400,7200,7000]'

# 4c./4d. The same two, with an oracle config. This CLI version cannot take
#    `Option<OracleConfig>` as inline JSON — it answers
#    "Expected type optional user-defined type 'OracleConfig', but received:
#    '{...}'" — so the value goes in a file and is passed with
#    `--oracle_config-file-path` (underscores, as printed by the CLI's own
#    suggestion):
cat > /tmp/oracle.json <<'JSON'
{"oracle_contract":"CCSSOHTBL3LEWUCBBEB5NJFC2OKFRC74OWEIJIZLRJBGAAU4VMU5NV4W","price_asset":"GBP","max_age_secs":86400,"denominated_amount":10000000000}
JSON
stellar contract invoke --id "$ID" --source-account deployer --network testnet --cost -- \
  initialize $BT --grade-price-bps '[10000,9000,7500]' \
  --oracle_config-file-path /tmp/oracle.json
```

`oracle_contract` above is Reflector's testnet fiat oracle (see
`reflector.rs`); `initialize` only records the config — it calls no oracle —
so any deployed contract address works for a cost measurement.

**Why the CLI, and not `soroban-cost-estimator`:** the estimator's `--arg`
can only express bool, integers and strings, so it still cannot encode
`initialize`'s three `Address`es, its `Vec<u32>` or its
`Option<OracleConfig>` (see the tooling finding above) — that limitation has
not changed. The CLI has no such gap: `--grade-price-bps '[10000,9000,7500]'`
for the vector and `--oracle_config-file-path` for the optional struct. This
is exactly the division of labour [`cost-estimation.md`](./cost-estimation.md)
recommends: the CLI is the source of record, the estimator an optional
reporting add-on.

## Reproducibility

WASM builds are verified reproducible in CI (`.github/workflows/test.yml`, job "Verify WASM build reproducibility"). The SHA-256 hash of `target/wasm32v1-none/release/harvestlock_escrow.wasm` must match between two consecutive clean builds. Current hash: see latest CI run.

## Offline WASM size budget

The committed release artifact is **82,567 bytes** (the build measured on
23 Nov 2026 above). To catch material size regressions without adding a live
network or another tool dependency, run this after `stellar contract build
--locked`:

```bash
bash .github/scripts/check_wasm_size.sh
```

The script compares
`target/wasm32v1-none/release/harvestlock_escrow.wasm` with
[`contracts/escrow/wasm_size.baseline`](../contracts/escrow/wasm_size.baseline),
prints the baseline, current size, and delta, and fails when growth exceeds
5 KiB (5,120 bytes). If a larger artifact is intentional, update the baseline
in the same PR and explain the increase; the check does not alter workflows.
Per the [PR template](../.github/PULL_REQUEST_TEMPLATE.md), a maintainer can
wire this command into `.github/workflows/test.yml`.

## Supply-Chain Health

`cargo deny check advisories bans` runs in CI to catch yanked or unmaintained dependencies. See `deny.toml` for configuration. As of this writing, the dependency tree is minimal (`soroban-sdk = "29.0.1"` only) and passes all checks cleanly.
