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
call site: **the contract uses `env.storage().persistent()` zero times,
and `env.storage().temporary()` zero times.** All contract state lives
in **instance storage** (`env.storage().instance()`), and that turns the
audit from "N entries, N TTL strategies" into something much simpler —
and much worse before this fix:

1. There are exactly three keys — `DataKey::Commitment`,
   `DataKey::Allocation`, `DataKey::OracleConfig` — and *all three* are
   fields of a single `ContractData` ledger entry (the contract instance
   entry itself). Instance storage isn't one ledger entry per key; the
   whole key/value map rides inside the instance. There is also no
   separate entry for "pending-dispute state" — `dispute_pre_status`
   and `dispute_deadline` are fields of the `Commitment` struct.
2. Consequently there is exactly **one TTL to manage** (the instance
   entry's), shared by all three keys, plus the contract's WASM code
   entry — which Soroban's `Instance::extend_ttl` bumps *together with*
   the instance entry by design.
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
- **Invoking the contract never touches its TTL.** After 3,996 ledgers
  of silence, `get_ttl()` is exactly the deploy value minus the
  dormancy; a successful `get_status()` before the fix left it
  unchanged (probe behavior; the same decay is asserted post-fix
  *above* the bump threshold in
  `locked_commitment_survives_long_dormancy_via_read_path_ttl_bumps`).
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

One instance entry holds everything; rows are the *logical* keys so the
table still answers the issue's "every key" framing.

| Ledger entry / key | Durability | Written at (all sites) | Read at | TTL before #98 | TTL now |
|---|---|---|---|---|---|
| Contract instance entry — holds `DataKey::Commitment` | instance (Persistent durability, entry-scoped TTL) | `initialize` (first `set`), `save()` from `lock`, `release_advance_*`, `mark_checkpoint`, `claim_*`/`reclaim_*` (via `claim_tranche`/`reclaim_tranche`/`open_tranche`), `ready_for_delivery`, `fund_remainder`, `expire_remainder_window`, `reclaim_on_nondelivery`, `confirm_delivery`, `settle`, `resolve_fx_shortfall`, `fund_fx_shortfall`, `expire_fx_shortfall_window`, `cancel`, `reassign_buyer`, `flag_dispute`, `resolve_dispute`, `expire_dispute_window` | `load()` — every public function except `initialize`; `initialize`'s duplicate-guard `has` | deploy-time `min_persistent_entry_ttl`, **never extended** — archived if not touched within that window, escrow stranded | re-rented to network `max_ttl` whenever below `max_ttl/2` by `bump_instance_ttl`, called from `load()` (every read *and* every write path) and from `initialize` |
| instance entry — `DataKey::OracleConfig` | instance | `initialize` only (when `Some`; immutable after) | `get_oracle_config`, `oracle_rate`, `settle` (`has` guard), `resolve_fx_shortfall` | same single TTL; same gap | covered by the same bump (same entry) |
| instance entry — `DataKey::Allocation` | instance | `set_allocation` only (one-time) | `get_allocation` | same single TTL; same gap | covered by the same bump (same entry) |
| Contract WASM code entry (`ContractCode`) | persistent | `install`/`deploy` | host loads it on every call | deploy-time minimum, never extended | `Instance::extend_ttl` rents instance **and code** together — asserted in `dormant_instance_is_rescued_by_the_first_call` |
| Token balances (escrow's holdings) | *not this contract's entry* — lives in the token contract's own ledger state | `lock`/`fund_*` transfers | `balance()` calls in `settle`/`expire_*`/`reclaim_*`/`cancel` | unaffected by escrow archival — this is *why* archival strands funds instead of destroying them | unchanged; still the token contract's own TTL story |

## The fix (what changed in `lib.rs`)

- New internal `bump_instance_ttl(env)`: `let max_ttl =
  env.storage().max_ttl(); env.storage().instance().extend_ttl(max_ttl
  / 2, max_ttl);`
- Called at the top of `load()` — which every public entry point but
  `initialize` already funnels through, reads (`get_status`,
  `get_commitment`, `get_allocation`, `get_oracle_config`, `oracle_rate`)
  and mutations alike — and once inside `initialize` after its writes
  (`initialize` deliberately can't call `load`, there's nothing stored
  yet).
- Strategy: re-rent **only when remaining TTL < half the network
  maximum**, topping back to the maximum. Consequences: (a) a dormant
  commitment is rescued by the *first* touch of any kind, including the
  API's routine permissionless reads; (b) once topped, steady-state
  calls cost no rent at all (no-op above the threshold — asserted in
  the second test); (c) rent is paid at most once per half-max lifetime
  per contract, i.e. roughly twice per network `max_entry_ttl` window
  even for the busiest deals. No state-machine logic or auth is
  touched, so all 191 pre-existing tests kept passing unchanged (see
  Tests).

## Residual risk, documented instead of hidden

The fix makes *any* activity keep the deal alive, but it cannot make the
entry immortal: a commitment that receives **zero calls of any kind for
more than the network's maximum instance lifetime** still gets archived.
That is inherent to instance storage under Soroban's state archival —
no in-contract call can extend an entry past the network max — so the
remaining handling is operational:

1. **Prevention**: the app/API already polls `get_status` /
   `get_commitment` for cache display; with `load()` bumping, that
   polling *is* the keeper job, for free up to the threshold. A
   dedicated cron calling `get_status` on every live commitment (or
   `stellar contract extend --id <contract> --ledgers-to-extend N
   --source <keeper>` from outside) bounds dormancy below the max
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
