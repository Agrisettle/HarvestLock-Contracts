# Persistent storage TTL / archival audit (#98)

Scope: every ledger entry this contract creates or touches, checked
against Soroban's state-archival rules — does each entry get its TTL
extended somewhere, or can a legitimately dormant commitment have its
own state archived out from under it? Method: a line-by-line read of
every `env.storage()` call in `contracts/escrow/src/lib.rs` (there is no
other contract source — `reflector.rs` only makes cross-contract calls,
and `api/` doesn't exist in this repo), plus two host-level probes run
as tests against the pinned `soroban-sdk = 28.0.0` (see the Tests
section). This audit is *pre-fix* state → *fix* → *residual risk*, in
that order, because the headline finding was a real gap.

## Headline finding: the structure differs from the issue's framing — and the gap is real anyway

The issue asked to enumerate "every key written via
`env.storage().persistent()`". The honest answer after grepping every
call site: the commitment and oracle configuration use instance storage,
while the allocation ledger uses one separate persistent entry. The audit
therefore changes from "N entries, N TTL strategies" into something much simpler —
and much worse before this fix:

1. `DataKey::Commitment` and `DataKey::OracleConfig` are fields of the
   instance entry; `DataKey::Allocation` is a separate persistent entry.
   Instance storage isn't one ledger entry per key; the whole key/value map
   rides inside the instance. There is also no
   separate entry for "pending-dispute state" — `dispute_pre_status`
   and `dispute_deadline` are fields of the `Commitment` struct.
2. Consequently there are two TTLs to manage: the instance/code entry's
   TTL and the allocation entry's TTL. Both are extended on the normal
   access path.
3. The bad part: **nothing extended that TTL anywhere before this
   change.** No call site, no read path, no deployer hook. And Soroban
   does *not* auto-extend a contract instance's TTL merely because the
   contract was invoked — this was verified empirically against the
   `env-host`, not assumed (see "Empirical facts" below). A deployed,
   initialized, then silent escrow — e.g. `Draft` or `Locked` with no
   deadline forcing anyone to act, exactly the case #98 named — decays
   to archival with its escrow sitting in the token contract's own
   books, and every function of the escrow instance starts failing the
   moment its entry is gone. That is a real gap: state archived while
   still needed, funds reachable-by-no-one until a restore.

## Empirical facts pinned by tests (host behavior, not folklore)

From `soroban-sdk` 28.0.0 / `soroban-env-host` 28.0.2, verified by
running code in this repo's own test suite:

- **Deploy/`register` gives the instance entry the network's
  `min_persistent_entry_ttl`** and nothing more. In the test host's
  default `LedgerInfo` that is 4,096 ledgers (`ttl at deploy ==
  min_persistent − 1`, asserted in
  `dormant_instance_is_rescued_by_the_first_call`).
- **A simulated invocation never changes the ledger TTL.** Contract
  execution during simulation may call `extend_ttl`, but simulation does
  not submit or commit those writes. Testnet `getLedgerEntries` readings
  around a simulated `get_status` are recorded below. A submitted,
  successful invocation can commit the `load()` extension; a failed
  transaction rolls it back.
- **TTL decays 1:1 with ledger closes** while dormant.
- `env.storage().instance().extend_ttl(threshold, extend_to)` only acts
  when the entry's remaining TTL is *below* `threshold`, and the new
  `live_until_ledger` is clamped by the network's `max_live_until_ledger`
  (`= sequence + max_entry_ttl − 1`). In the test host, `max_entry_ttl`
  is 6,312,000 ledgers.
- The real-network numbers are whatever the target network configures —
  `min_persistent_entry_ttl`, `max_entry_ttl`, and the archival/bucket
  window are `getLedgerEntries`-readable ConfigSettings on any live
  network (testnet/pubnet values change with protocol config votes;
  this sandbox has no RPC access, so the doc deliberately doesn't print
  a pubnet number as fact). The strategy below is parameterized off
  `env.storage().max_ttl()` for exactly that reason: the contract never
  hard-codes a network's numbers.

## Entry-by-entry table (post-fix state)

The commitment and oracle configuration share the instance entry; the
allocation ledger is deliberately isolated in its own persistent entry. Rows
still identify the logical keys so the table answers the issue's "every key"
framing.

| Ledger entry / key | Durability | Written at (all sites) | Read at | TTL before #98 | TTL now |
|---|---|---|---|---|---|
| Contract instance entry — holds `DataKey::Commitment` | instance (Persistent durability, entry-scoped TTL) | `initialize` (first `set`), `save()` from `lock`, `release_advance_*`, `mark_checkpoint`, `claim_*`/`reclaim_*` (via `claim_tranche`/`reclaim_tranche`/`open_tranche`), `ready_for_delivery`, `fund_remainder`, `expire_remainder_window`, `reclaim_on_nondelivery`, `confirm_delivery`, `settle`, `resolve_fx_shortfall`, `fund_fx_shortfall`, `expire_fx_shortfall_window`, `cancel`, `reassign_buyer`, `flag_dispute`, `resolve_dispute`, `expire_dispute_window` | `load()` — every public function except `initialize`; `initialize`'s duplicate-guard `has` | deploy-time `min_persistent_entry_ttl`; archived if it expires before being touched by a submitted transaction | a submitted, successful call that reaches `load()` re-rents to network `max_ttl` whenever below `max_ttl/2`; `initialize` does likewise. Simulated invocations do not persist the extension |
| instance entry — `DataKey::OracleConfig` | instance | `initialize` only (when `Some`; immutable after) | `get_oracle_config`, `oracle_rate`, `settle` (`has` guard), `resolve_fx_shortfall` | same single TTL; same expiry risk | covered by the same bump when a submitted invocation commits |
| persistent entry — `DataKey::Allocation` | persistent | `set_allocation` only (one-time) | `get_allocation` | not applicable before migration | a submitted, successful call reaching `load()` re-rents to `max_ttl` when below `max_ttl/2`; simulation does not persist the extension |
| Contract WASM code entry (`ContractCode`) | persistent | `install`/`deploy` | host loads it on every call | deploy-time minimum, never extended | `Instance::extend_ttl` rents instance **and code** together — asserted in `dormant_instance_is_rescued_by_the_first_call` |
| Token balances (escrow's holdings) | *not this contract's entry* — lives in the token contract's own ledger state | `lock`/`fund_*` transfers | `balance()` calls in `settle`/`expire_*`/`reclaim_*`/`cancel` | unaffected by escrow archival — this is *why* archival strands funds instead of destroying them | unchanged; still the token contract's own TTL story |

## TTL target decision

The implementation keeps topping the instance up to `max_ttl`; it does not
cap the extension at `delivery_deadline` or another stored deadline. A
deadline-derived horizon is not a reliable upper bound on a commitment's
useful life: a `Draft` can remain open until someone locks it, and a dispute
can be resolved by agreement without consuming the lapse cap, then opened
again. Those paths can move the last reachable action later without a fixed
maximum. Capping the TTL at a currently-known deadline could therefore archive
a still-actionable commitment. Max-TTL remains the conservative choice until
the lifecycle has a hard end time.

The rent tradeoff is material. The testnet cost record in
[`gas-costs.md`](./gas-costs.md#ttl-extension-rent-tradeoff) measured **4,092,383
stroops gross rent** for the fresh-instance `initialize` extension to max TTL.
For scale, if rent were linear in TTL and a ledger closed every five seconds,
120 days is 2,073,600 ledgers, or **32.9%** of the test host's 6,312,000-ledger
max TTL. A hypothetical extension of that length would be about **1,344,000
stroops**, roughly **2,748,000 stroops less** than the measured max-TTL
extension. This is an illustrative pro-rata estimate, not a second measured
transaction: actual rent depends on the network's rent settings, ledger
interval, entry size, current TTL, and any safety/window allowance. It also
does not cover the unbounded lifecycle cases above.

## The fix (what changed in `lib.rs`)

- New internal `bump_instance_ttl(env)`: `let max_ttl =
  env.storage().max_ttl(); env.storage().instance().extend_ttl(max_ttl
  / 2, max_ttl);` plus the same threshold strategy for the persistent
  allocation entry.
- Called at the top of `load()` — which every public entry point but
  `initialize` already funnels through, reads (`get_status`,
  `get_commitment`, `get_allocation`, `get_oracle_config`, `oracle_rate`)
  and mutations alike — and once inside `initialize` after its writes.
  The extension reaches the ledger only when the invocation is submitted
  successfully; a simulated read is discarded.
- Strategy: re-rent **only when remaining TTL < half the network
  maximum**, topping back to the maximum. The choice to retain max-TTL,
  rather than cap at a known deadline, is explained above. Consequences:
  (a) a dormant commitment is rescued by the first submitted successful
  invocation after it reaches the threshold; API `simulateRead` polling does
  not rescue it; (b) once topped, steady-state
  calls cost no rent at all (no-op above the threshold — asserted in
  the second test); (c) rent is paid at most once per half-max lifetime
  per contract, i.e. roughly twice per network `max_entry_ttl` window
  even for the busiest deals. No state-machine logic or auth is
  touched, so all 191 pre-existing tests kept passing unchanged (see
  Tests).

## Storage layout

All three logical keys share the contract instance entry, so the size that
matters for the network limit is the serialized `LedgerEntry`, not just the
size of an individual Rust value. The measurement test uses
[`soroban_sdk::xdr::ToXdr`](https://docs.rs/soroban-sdk/29.0.1/soroban_sdk/xdr/trait.ToXdr.html)
for each stored value and the ledger snapshot's XDR for the complete instance
entry. The test-host limit is **65,536 bytes**
(`max_contract_data_entry_size_bytes` in the pinned SDK's resource-limit
configuration; see the [official SDK source](https://github.com/stellar/rs-soroban-sdk/blob/v29.0.1/soroban-sdk/src/testutils/cost_estimate.rs#L150-L160)).
Network configuration can change, so the 65,536-byte value is the documented
reference for this pinned SDK and the live network should still be checked
before a deployment.

The allocation is now its own persistent entry. “Instance entry” includes the
ledger-entry framing; the final column is the combined footprint of the
instance and allocation entries.

| Stage | `Commitment` | `OracleConfig` | Persistent allocation | Instance entry | Total footprint |
|---|---:|---:|---:|---:|---:|
| Fresh `Draft` | 1,348 B | — | — | 1,488 B | 1,488 B |
| With oracle config | 1,348 B | 188 B | — | 1,708 B | 1,708 B |
| With full allocation | 1,348 B | 188 B | 20,012 B | 1,708 B | 21,752 B |
| With 16-entry grade schedule | 1,452 B | 188 B | 20,012 B | 1,856 B | 21,868 B |

The test keeps the worst-case instance below **50%** of the 65,536-byte
reference limit (32,768 B), and also asserts component bounds of 4,096 B for
`Commitment`, 512 B for `OracleConfig`, and 24,000 B for a 200-member
allocation. The 200-member allocation is the dominant addition: it contributes
about 20 KB by itself. Expanding the grade schedule from the ordinary
three-entry fixture to the allowed sixteen entries adds 104 B to the
`Commitment`; the oracle config is comparatively small.

## Residual risk, documented instead of hidden

The fix makes *any* activity keep the deal alive, but it cannot make the
entry immortal: a commitment that receives **zero calls of any kind for
more than the network's maximum instance lifetime** still gets archived.
That is inherent to instance storage under Soroban's state archival —
no in-contract call can extend an entry past the network max — so the
remaining handling is operational:

1. **Prevention**: API `get_status` / `get_commitment` polling is simulated
   and does not commit the `load()` extension; it is **not** a keeper.
   Run the [TTL keeper tool](../tools/ttl-keeper/README.md) from a funded
   cron identity instead. It reads both the contract instance and
   Wasm-code TTL, reports contracts below the configured threshold, and
   submits `stellar contract extend` transactions for those contracts.
   A successful submitted contract invocation that reaches `load()` also
   commits its in-contract extension. This bounds dormancy below the max
   lifetime and prevents eviction outright.
2. **Recovery if eviction already happened**: the archived instance
   entry stays restorable for the network's archive/bucket window —
   `stellar contract restore` (a subcommand of the same CLI this repo's
   CI uses) brings the entry back *with* its instance storage (the
   `Commitment` included), after which the deal resumes exactly where it
   stopped, because the token balance never left. This is the whole
   reason escrow stranding is recoverable rather than fatal.
3. **After the restore window closes**: the state is gone permanently —
   the escrow's token balance stays in the token contract but this
   contract can no longer be called. Nothing about the escrow design
   could recover from that except redeploying the instance (fresh id, no
   state). This is called out because it is the actual worst case of
   the gap #98 asked about; the fix shrinks its probability to
   "nobody ever touches a live deal for longer than the network's
   maximum instance lifetime", which prevention step 1 bounds.

## What the failed-transaction case costs

`extend_ttl` from `load()` runs before guards can still reject (e.g.
`InvalidState`): in Soroban a *failed* transaction's whole footprint
rolls back, TTL bumps included, so rejected calls pay no rent and leave
no partial extension. The same rollback property is what makes
`confirm_delivery`'s check-before-mutate ordering (#95) safe under both
old and hypothetical-new orderings.

## Tests

- **Testnet simulation probe (25 Nov 2026)** — Deployment 13's live
  commitment `CBF4LWV2NHI5I64OVKAVLEF6O3D2SEFV4NZ4MS74QQTUF2R5S7F4CI3N`
  returned `Settled` from `stellar contract invoke ... --send=no --
  get_status`. RPC `getLedgerEntries` reported instance
  `liveUntilLedgerSeq = 9,008,833` at `latestLedger = 5,915,208` before
  simulation and the same `liveUntilLedgerSeq = 9,008,833` at
  `latestLedger = 5,915,213` after. The returned status was `Settled`;
  the live-until value did not move while ledgers advanced. This confirms
  the simulated polling path does not extend live TTL.
- `dormant_instance_is_rescued_by_the_first_call` — a deployed instance
  starts at exactly `min_persistent_entry_ttl − 1` (test host: 4,095),
  decays 1:1 through 3,996 ledgers of silence to 99 with no auto-bump,
  and the first call that reaches it (`initialize`) re-rents the entry —
  *and its WASM code entry* — to the full network `max_ttl`.
- `locked_commitment_survives_long_dormancy_via_read_path_ttl_bumps` —
  after `lock`, one permissionless `get_status` read past the half-max
  dormancy point re-rents the whole instance to max, a subsequent read
  while still above the threshold deliberately does *not* re-rent
  (amortized rent), and the rescued `Locked` commitment is still fully
  functional (status and escrowed balance intact).

Both live in `contracts/escrow/src/test.rs`; run with `cargo test
--locked` from `contracts/escrow`.

## Files

- `contracts/escrow/src/lib.rs` — `bump_instance_ttl`, calls from
  `load()` and `initialize()`, module-doc paragraph, and the doc
  comments recording the #95 ordering conclusion and the #96
  abandoned-at-Draft answer (no funds ever moved at that stage).
- `docs/minimal-example.md` — the #97 walkthrough, whose step table
  notes storage lifetime as a standing obligation of *any* commitment,
  minimal or not.
