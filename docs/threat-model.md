# Threat model — HarvestLock escrow contract

What this contract promises, who can make that promise wrong, and which of
those attacks are defended against, accepted as a known risk, or explicitly
out of scope. This file exists because the reasoning is currently scattered
across `contracts/escrow/src/lib.rs`'s module and function doc comments and
[`HANDOFF.md`](../HANDOFF.md)'s design-decisions section — real conclusions,
but a reviewer has to reassemble them from four places to see the whole
picture. Without the assembly, nobody can tell an oversight from an accepted
risk. [`SECURITY.md`](../SECURITY.md) says what is *reportable*; this says
what was already known and decided when the code was written.

Scope: the contract in [`contracts/escrow`](../contracts/escrow) as it stands
on `main` (the state machine through Deployment 10 plus the later additions
recorded in HANDOFF.md), exercised on Stellar **testnet** only. Where a
statement is about something not built yet (the `api/` layer, reputation
enforcement), it says so.

Ground truth is the code; the spec is the PRD
([`docs/PRD.md`](https://github.com/Agrisettle/HarvestLock/blob/main/docs/PRD.md)
in the sibling `HarvestLock` repo). Two companion docs go deeper on parts of
this one: [`authorization-matrix.md`](./authorization-matrix.md) (every
`require_auth()` site mapped to the PRD) and [`dispute-scope.md`](./dispute-scope.md)
(what the dispute trio covers and deliberately doesn't). Where this doc and
the PRD disagree about intent, the PRD wins; where they disagree about what
the contract *does*, the code wins.

## The one thing that must not go wrong

This contract's whole job is that **every stroop leaves the escrow only
where the terms fixed at lock-in say it should go** — no more, no less, to
no one else. That gives the analysis its asymmetry, the same way the
fail-closed direction does for a scanner:

- A call that is **wrongly refused** leaves the money sitting in the
  contract. Funds stuck in a contract are visible, contestable off-chain,
  and recoverable by a later correct call or a deadline path. Annoying; not
  fatal.
- A call that **wrongly succeeds and moves money** is irreversible on the
  ledger. Nobody can un-send a Stellar transaction.

So every verdict below follows from one rule: hard reverts are cheap and
accepted throughout; an irreversible misdirection — funds paid to a party or
in an amount the fixed terms didn't entitle them to — is the line this
document will not accept being crossed.

| Asset at risk | Why it matters | Failure direction |
| --- | --- | --- |
| The escrowed balance (deposit + funded remainder + FX top-ups) | A misdirected payout can't be reversed | **Unsafe** |
| The settlement outcome (`settlement_bps`, the FX number) | A wrong number *is* a wrong payout | **Unsafe** |
| Contract liveness (money stuck behind a halted state machine) | Refusals and freezes keep funds in the contract | Recoverable: visible, deadline-bounded |
| The allocation ledger (transparency + NDPA unlinkability) | A wrong record causes off-chain harm; a linkable hash causes privacy harm | Detective, off-chain consequences |
| Observability (`status_changed` events, public state) | An indexer can't detect what it can't see | Detective |

## Trust boundaries

```
 buyer key ────────────┐
 cooperative key ──────┤ require_auth (Soroban auth entries,
 warehouse op key ─────┤  method+argument bound, ledger-expiring)
 3rd-party caller ─────┘         │
                                 ▼
                    ┌────────────────────────────┐
 Reflector oracle ──▶│   escrow contract (per-   │──▶ token transfers ──▶ buyer /
 (cross-contract     │   commitment instance,    │    (SAC / native XLM)   cooperative
  read, freshness-   │   no admin, no upgrade)   │
  bounded)           └────────────────────────────┘
                                 ▲
                                 │ read-only (get_status / get_commitment /
                                 │ oracle_rate / get_allocation / …)
                    indexers, API layer, observers — no keys, no write path

 OFF-CHAIN, no on-chain authority at all:
   HarvestLock API / maintainers (build & submit txs, censor users off-chain,
   enforce reputation bars) · wallet/social-recovery layers · warehouse
   operator's appeals process · Postgres salt↔identity map (NDPA erasure)
```

Two boundaries matter most. First, **every arrow into the contract that
moves state goes through `require_auth`** — there is no admin key, no
upgrade path, and no privileged entry point, so the authentication surface
*is* the three parties' keys plus permissionless calls. Second, **the
oracle arrow is the only one carrying untrusted external data**, and it is
bounded by a caller-chosen freshness bound rather than trusted outright.

## Actors

| Actor | What they control | What they can do to this contract | What they cannot do |
| --- | --- | --- | --- |
| **Buyer** | Their Stellar key; the deal's terms at `initialize` (amounts, bps, windows, token, oracle config) | `initialize`, `lock`, `fund_remainder`, `fund_fx_shortfall`, `reclaim_advance_*`, `reclaim_on_nondelivery`; co-sign `cancel` / `reassign_buyer`; flag a dispute | Unilaterally cancel; confirm delivery; claim an advance; withhold `settle` once its conditions are met; claw back an advance the cooperative already claimed; amend terms after `initialize` (no setter exists) |
| **Cooperative** | Their Stellar key; the member list authored pre-`lock` | `set_allocation` (once, pre-`lock`), `claim_advance_*`, `ready_for_delivery`; co-sign `cancel` / `reassign_buyer`; flag a dispute | Attest checkpoint or delivery; cancel alone; force `settle` early; change or amend the allocation after `lock`; receive the remainder before the buyer funds it |
| **Warehouse operator** | Their Stellar key; their own off-chain grading/appeals process | `mark_checkpoint`, `confirm_delivery` (the attestation that unlocks settlement); flag a dispute | Move funds — the contract never pays the operator anything; adjudicate disputes on chain; alter `grade_price_bps` (fixed at `initialize`); be over-paid or directed anywhere by any call |
| **Any third party** (stranger, indexer, griefer) | Nothing on chain | Call the 7 permissionless functions, which can only *complete a check the deal already fixed*; call any view | Flag a dispute (`NotAParty`); call any gated function; supply an outcome, destination, or amount to any call; read anything secret — no confidentiality is claimed, none exists |
| **Reflector oracle** (external) | Its own published quotes | Supply the rate `oracle_rate` / `resolve_fx_shortfall` read | Choose the staleness bound, the asset symbol, or the oracle address — all fixed by the buyer at `initialize`; supply a stale quote as if fresh (`OracleStale`, a hard error) |
| **HarvestLock / API maintainers** (off-chain) | Which transactions get built and submitted; the reputation store | Build, sign-request, and submit transactions on behalf of users; censor users *off chain*; enforce the default/forfeiture bars app-side | Sign *as* any party, pause, upgrade, drain, or redirect the contract — no HarvestLock address exists anywhere in the contract (PRD §4.5) |
| **Member farmers** (record subjects) | Their off-chain identity | Nothing — no member key is ever a contract principal | Read or alter their on-chain `member_hash` row; be paid by the contract (v1 `settle` pays the cooperative a lump sum) |
| **Deployers / maintainers** | Source code, CI, the WASM hash | Ship a new contract version; review PRs | Change a deployed instance — there is no upgrade entrypoint, by design |
| **Token contract** (SAC / native XLM) | Its own transfer semantics | Halt every call that touches it (its failure unwinds the whole invocation) | Be swapped after `initialize` (address stored at creation) |

Note on being named: the cooperative and warehouse operator **never sign at
`initialize`** — only the buyer does. A commitment can therefore be created
naming a cooperative that never agreed to anything. That is harmless by
construction: being named imposes no obligation and moves no money, because
every action those parties can take requires their own later signature, and
the only funds at risk before then are the buyer's own.

## The authorization surface in one screen

29 public functions, 5 shapes, **zero un-gated fund-moving paths** — 13
single-party gates, 7 permissionless state changes, 3 multi-party mutual
actions, 1 caller-authenticated flag, 5 no-auth views. The full table with
PRD citations is [`authorization-matrix.md`](./authorization-matrix.md) §1;
this doc only assumes its central invariant:

> A permissionless call can only complete a fact the deal already fixed at
> lock-in. It never accepts an outcome parameter, never redirects a
> destination, and never grants anyone anything they weren't already
> entitled to under the agreed terms.

CI enforces the mechanical half of keeping this honest: every `pub fn` in
`lib.rs` must carry a doc comment naming its auth shape
(`.github/scripts/check_auth_docs.sh`, run on every PR).

## Attack classes

Format: **verdict**, then the reason, then — for anything defended — the
test that holds it. "Accepted" means the exposure is known, bounded, and the
reason it is not fixed is stated; not that nobody thought about it.

### Fund movement and authorization

#### 1. A call moves funds without the named party's signature
**Verdict: defended.** Every fund-moving or fund-unlocking path sits behind
`require_auth()` of the party whose money or entitlement it is (call sites:
`lib.rs` — `initialize`, `set_allocation`, `lock`, `claim_tranche`,
`reclaim_tranche`, `mark_checkpoint`, `ready_for_delivery`, `fund_remainder`,
`reclaim_on_nondelivery`, `confirm_delivery`, `fund_fx_shortfall`,
`cancel`, `reassign_buyer`, `flag_dispute`, `resolve_dispute`). Under
Soroban's enforcing auth manager an unsigned invoke aborts in the host,
before any contract code runs. Tests pinning the gates:
`fund_remainder_requires_buyer_auth`,
`fund_fx_shortfall_requires_buyer_auth`,
`reclaim_on_nondelivery_requires_buyer_auth`,
`set_allocation_requires_cooperative_auth`,
`ready_for_delivery_requires_cooperative_auth`,
`confirm_delivery_requires_warehouse_operator`,
`cancel_requires_both_buyer_and_cooperative_auth`,
`reassign_buyer_requires_current_buyer_cooperative_and_new_buyer_auth`,
`resolve_dispute_requires_all_three_parties_auth`.
Honest coverage note: those nine assert either the recorded auth trace
naming the right address, or (under `mock_auths`, where rejection is
observable) that a wrong signer fails. The remaining eight gates —
`initialize`, `lock`, `mark_checkpoint`, `claim_advance_1/2`,
`reclaim_advance_1/2`, and `flag_dispute`'s own flagger signature — are
held by the identical mechanism and host enforcement but have no dedicated
rejection test; their per-function inventory is authorization-matrix §1, and
`flag_dispute_rejects_a_non_party` covers the party check on the last one.

#### 2. A stranger freezes, steers, or forges a dispute
**Verdict: defended.** `flag_dispute` requires the flagger's own auth *and*
that the flagger is the buyer, cooperative, or warehouse operator — any
other address gets `NotAParty` before the auth check is even reached. No
other function takes a caller-supplied party, destination, or outcome at
all. Tests: `flag_dispute_rejects_a_non_party`,
`flag_dispute_rejects_from_draft`,
`flag_dispute_rejects_after_settlement`.

#### 3. Partial, duplicated, or forged multi-party consent
**Verdict: defended.** `cancel`, `reassign_buyer`, and `resolve_dispute`
route through `require_once_each`, which demands one `require_auth()` per
*distinct* address in the same invocation — two-of-three is never enough,
and a signature staged for one method or one argument set cannot be pointed
somewhere else. Tests:
`cancel_requires_both_buyer_and_cooperative_auth`,
`reassign_buyer_requires_current_buyer_cooperative_and_new_buyer_auth`,
`resolve_dispute_requires_all_three_parties_auth`,
`resolve_dispute_requires_three_distinct_signatures_when_parties_are_distinct`,
`two_of_three_identical_parties_still_require_the_distinct_partys_auth`,
`all_three_identical_parties_accept_only_that_principals_signature`,
`auth_entry_for_one_method_cannot_authorize_a_different_method`,
`auth_entry_for_one_argument_set_cannot_authorize_a_different_argument_set`.

#### 4. Replay of a staged auth entry after its expiry
**Verdict: defended.** A staged Soroban auth entry whose
`signature_expiration_ledger` lapses is rejected by the host atomically with
the call — no partial state is written. Test:
`expired_staged_auth_entry_fails_cleanly_with_no_partial_state`.

#### 5. One principal holding two named roles breaks or bypasses the gate
**Verdict: defended** (this was a real shipped bug, fixed). Soroban's auth
manager rejects a *second* `require_auth()` for the same address in one
frame, so a cooperative that is also the buyer, or a merged three-identical-
parties commitment, used to hard-fail (`Auth/InvalidAction`) — turning a
valid single-signature authorization into a denial of service against
honest users. `require_once_each` deduplicates by address. Tests:
`cancel_succeeds_when_buyer_and_cooperative_share_one_address`,
`reassign_buyer_succeeds_when_buyer_and_cooperative_share_one_address`,
`all_three_identical_parties_accept_only_that_principals_signature`,
`two_of_three_identical_parties_still_require_the_distinct_partys_auth`.

### State-machine integrity

#### 6. `settle` runs before the deal's conditions are met
**Verdict: defended.** `settle` requires `Delivered`, both tranches
resolved, and — for oracle commitments — FX resolution plus a funded
shortfall. Tests: `cannot_settle_before_delivery_confirmed`,
`settle_blocked_if_advance_1_never_resolved`,
`settle_blocked_if_advance_2_never_resolved`,
`zero_advance_bps_still_requires_explicit_resolution_before_settle`,
`settle_blocked_until_fx_resolved`,
`settle_blocked_while_fx_shortfall_unfunded`.

#### 7. `settle` silently sweeps a lapsed-but-unclaimed tranche to the
cooperative
**Verdict: defended — this was the one genuine bug a prior self-audit
caught before it shipped.** The first version swept the balance to the
cooperative, which would have handed a buyer's already-vested reclaim right
away on nothing more than the buyer's inactivity. Now every stroop's
destination is the result of an explicit `claim_*`/`reclaim_*` call, never
inferred (`TrancheUnresolved = 12`). The tests above hold it, and
Deployment 2 proved the *deployed* WASM rejects it on-chain with
`Error(Contract, #12)` — not just the native test build. See `settle`'s doc
comment for why this must not be "simplified" back.

#### 8. Delivery confirmed while the buyer's money isn't in escrow
**Verdict: defended.** `confirm_delivery` needs `ReadyForDelivery` *and*
`remainder_funded`, checked as two separate guards so the caller sees
exactly which precondition failed. Tests:
`cannot_confirm_delivery_before_ready_for_delivery`,
`cannot_confirm_delivery_before_remainder_funded`.

#### 9. Double-execution of any one-time action
**Verdict: defended.** Every one-time path checks its flag/status before
acting: `cannot_initialize_twice`, `cannot_set_allocation_twice`,
`cannot_claim_advance_1_twice`, `cannot_reclaim_advance_1_twice`,
`cannot_claim_after_buyer_already_reclaimed`,
`cannot_reclaim_after_cooperative_already_claimed`,
`cannot_fund_remainder_twice`, `cannot_mark_checkpoint_twice`,
`cannot_cancel_twice`, `cannot_resolve_fx_shortfall_twice`,
`cannot_fund_fx_shortfall_twice`, `flag_dispute_rejects_a_second_flag`,
`cannot_expire_fx_shortfall_window_once_funded`.

#### 10. Out-of-order or skipped transitions
**Verdict: defended.** Every state-changing call matches a specific
required status, so nothing can jump the machine:
`cannot_release_advance_1_before_lock`,
`cannot_claim_advance_1_before_it_is_opened`,
`cannot_call_ready_for_delivery_before_advance_2_released`,
`cannot_fund_remainder_before_ready_for_delivery`,
`cannot_reclaim_on_nondelivery_from_draft`,
`cannot_reclaim_on_nondelivery_after_delivered`,
`cannot_cancel_after_delivery_confirmed`,
`cannot_reassign_buyer_after_delivery_confirmed`,
`resolve_dispute_rejects_when_not_disputed`.

#### 11. A deadline fired before it passed, or after it was cured
**Verdict: defended.** Every deadline path checks the clock *and* the cure
condition, and the claim/reclaim boundary is pinned exactly (a claim at the
deadline instant still succeeds; reclaim is only possible strictly after),
so there is no ambiguity window for a race:
`cannot_reclaim_advance_1_before_window_passes`,
`claim_exactly_at_deadline_still_succeeds`,
`cannot_fund_remainder_after_window_passed`,
`cannot_expire_remainder_window_before_deadline`,
`cannot_expire_remainder_window_if_already_funded`,
`cannot_reclaim_on_nondelivery_before_deadline`,
`cannot_fund_fx_shortfall_after_the_window_passes`,
`cannot_expire_fx_shortfall_window_before_the_deadline`,
`cannot_expire_fx_shortfall_window_once_funded`,
`expire_dispute_window_rejects_before_the_deadline`.

### Dispute mechanism

#### 12. One party freezes a commitment forever
**Verdict: defended.** The freeze is unilateral but bounded: either
unanimous consent (`resolve_dispute`) or a timeout
(`expire_dispute_window`, deliberately permissionless so a refusing party
can't hold the commitment hostage) restores exactly the pre-dispute status.
Tests: `resolve_dispute_requires_all_three_parties_auth`,
`expire_dispute_window_is_permissionless`,
`expire_dispute_window_restores_pre_dispute_status_after_the_deadline`,
`dispute_resolve_and_expire_race_leaves_consistent_state_regardless_of_order`.

#### 13. The dispute path is used to decide fault or rewrite an outcome
**Verdict: defended by absence — and it is an explicit non-goal.** Both
resume paths restore `status` verbatim from `dispute_pre_status`; no fault
finding, payout split, or redirect-to-`Cancelled` parameter exists to accept.
If the parties agree off-chain to unwind, they call `cancel()` afterward,
exactly as they would have without a dispute. Tests:
`resolve_dispute_restores_the_exact_pre_dispute_status`,
`resolve_dispute_allows_normal_flow_to_continue_afterward`.

#### 14. The freeze doesn't actually freeze anything
**Verdict: defended, with one documented exception.** The freeze works by
construction for every *status-gated* call — none of them ever accepts
`Disputed` as a required status — and one test walks those calls to prove
it rather than asserting it in prose:
`flag_dispute_freezes_every_other_state_changing_call`. The exception is
`claim_advance_1/2` and `reclaim_advance_1/2`: `claim_tranche` /
`reclaim_tranche` guard only the tranche deadline, the one-shot flags, and
the party's auth, so a dispute does **not** block a claim or a reclaim. A
dispute therefore cannot be used to protect a tranche from the other side
(and equally cannot extend a window). See
[`dispute-frontrunning.md`](./dispute-frontrunning.md) §2.

#### 15. Deadlines keep ticking while a dispute is open
**Verdict: accepted risk, pinned by a test.** Freezing `status` does not
pause any *absolute* deadline already running (advance claim windows,
`remainder_deadline`, `delivery_deadline`, `fx_shortfall_deadline`). A
deadline that lapses mid-dispute becomes actionable the moment the dispute
clears. This is deliberate: shifting every deadline by the dispute duration
is real added complexity for a mechanism nobody has used yet, and the
project's bias is to build against real usage, not hypotheticals. The
limitation is proved, not merely stated:
`dispute_does_not_pause_an_already_ticking_deadline_known_limitation`.
Revisit if a real dispute ever collides with a deadline this way (see
`flag_dispute`'s doc comment and [`dispute-scope.md`](./dispute-scope.md) §4).

#### 16. A party flags a dispute to buy time
**Verdict: accepted risk, and the contract does not bound the
delay.** Any named party can pause the machine for up to
`remainder_window_secs` at a time, and because `expire_dispute_window`
restores the pre-dispute status while `flag_dispute` has no cooldown, bond,
or per-commitment counter, the flagger can re-flag immediately after each
expiry. The timeout bounds a *single* freeze, not the strategy. Two claims
this entry used to make need correcting: a pause *can* change an outcome,
because a cooperative can flag to block `reclaim_on_nondelivery` and use
the window to obtain a late `confirm_delivery` (entry 28,
[`dispute-frontrunning.md`](./dispute-frontrunning.md) §3.1); and the
freeze leaves other deadlines ticking (entry 15), so it buys time rather
than curing anything. The flag itself also generates no reputation event,
so a repeat staller accumulates no record. Full analysis:
[`dispute-stalling.md`](./dispute-stalling.md).

### Oracle and FX

#### 17. A stale, missing, or unconfigured oracle quote is used as if live
**Verdict: defended.** `oracle_rate` enforces PRD §16.3's staleness bound
and returns hard errors — never a silently old number — and the zero-bound
corner (which would accept nothing, or be gamed into accepting everything)
is rejected at `initialize`. Tests:
`oracle_rate_rejects_a_quote_older_than_max_age_secs`,
`oracle_rate_accepts_a_quote_exactly_at_the_staleness_boundary`,
`oracle_rate_fails_when_the_oracle_has_never_quoted_this_asset`,
`oracle_rate_fails_when_no_oracle_configured`,
`resolve_fx_shortfall_requires_oracle_configured`,
`initialize_rejects_a_zero_max_age_secs_oracle_config`,
`get_oracle_config_is_none_when_initialize_never_set_one`.

#### 18. Either side games the FX number by choosing when to resolve
**Verdict: partially defended — the number is final once set, but *which*
moment gets locked is a first-caller race.** `resolve_fx_shortfall` runs
exactly once (`FxAlreadyResolved`), only from `Delivered` with **both
tranches already resolved** so the comparison uses final figures rather
than a snapshot that could still move, and `settle` reads only the recorded
number — so the *loser of the race cannot re-resolve*. What the one-shot
guard does not do is remove the incentive to be the first caller at a
preferred quote: the two parties want opposite prices, there is no deadline
by which resolution must happen, and either can flag a dispute to deny the
other a favourable window. See
[`dispute-frontrunning.md`](./dispute-frontrunning.md) §3.3. Tests:
`cannot_resolve_fx_shortfall_twice`,
`cannot_resolve_fx_shortfall_before_tranches_are_resolved`,
`settle_blocked_until_fx_resolved`.

#### 19. A fresh-but-wrong oracle rate changes the payout
**Verdict: accepted risk, bounded.** Within `max_age_secs`, the contract
trusts the configured oracle verbatim. Bound: the buyer picks both the
oracle contract and the bound at `initialize`, resolution is one-time
(entry 18), a wrong-but-fresh rate can only move the payout by what the
oracle itself published at a legal moment, and any party can `flag_dispute`
to freeze the commitment before `settle` runs. Residual exposure: the
oracle contract's own correctness (including a compromised Reflector) is
upstream — see "Out of scope".

#### 20. The needed currency isn't quoted at all (NGN today)
**Verdict: defended by failure — the gap is explicit, not silent.** A
missing asset is `OraclePriceUnavailable`, never a substituted or guessed
rate. Reflector's live testnet fiat oracle doesn't quote NGN (confirmed by a
real `assets()` call, HANDOFF Deployment 8); `price_asset` works with
anything Reflector does quote and activates for NGN with no code change.
Tracked in HANDOFF's next steps as an upstream-feed gap.

#### 21. A third party seizes the timing of a permissionless first call
**Verdict: accepted risk, bounded.** Anyone can start a clock the state
machine has already authorized (`release_advance_*`, `settle`,
`resolve_fx_shortfall`, the `expire_*` sweeps) the moment it becomes
eligible.Because those calls take no outcome parameter and only complete pre-fixed
facts (authorization-matrix §2), a griefer cannot name a destination and
cannot redirect a payout to itself. It *can* still pick the instant of a
call, and two of those instants are not outcome-neutral: an
expiry sweep that runs before a vested `reclaim_advance_*` transfers the
lapsed tranche to the cooperative (the sweep does not require resolved
tranches, unlike `settle`), and the first caller of `resolve_fx_shortfall`
decides which oracle moment prices the deal. Both are documented in
[`dispute-frontrunning.md`](./dispute-frontrunning.md) §3.2–§3.3. Tests
pinning what permissionless *cannot* do:
`release_advance_1_opens_window_but_transfers_nothing`,
`expire_remainder_window_is_permissionless`,
`expire_dispute_window_is_permissionless`,
`cannot_expire_remainder_window_before_deadline`.

### Inputs, resources, and payout math

#### 22. Unbounded inputs used to exhaust storage or execution
**Verdict: defended.** The contract's only two unbounded vectors are capped
at call time — `MAX_GRADE_ENTRIES = 16`, `MAX_ALLOCATION_MEMBERS = 200`
(anchored to PRD §16.3's cooperative scale) — and a cap breach rejects
rather than truncates. Tests:
`initialize_rejects_a_grade_schedule_over_max_entries`,
`initialize_accepts_a_grade_schedule_at_max_entries`,
`cannot_set_allocation_with_more_than_max_members`,
`allocation_with_exactly_max_members_is_allowed`.

#### 23. Economically nonsensical or adversarial parameters
**Verdict: defended.** Rejected at `initialize` or at the write site, with
the bps addition done via `saturating_add` so an extreme `u32` input
compares cleanly instead of trapping:
`advance_bps_over_10000_rejected_at_initialize`,
`zero_claim_window_rejected_at_initialize`,
`zero_remainder_window_rejected_at_initialize`,
`zero_delivery_window_rejected_at_initialize`,
`initialize_rejects_zero_contracted_quantity`,
`initialize_rejects_empty_grade_schedule`,
`initialize_rejects_a_grade_priced_over_10000_bps`,
`cannot_set_allocation_with_an_empty_member_list`,
`cannot_set_allocation_with_shares_summing_over_10000_bps`,
`confirm_delivery_rejects_an_out_of_range_grade_index`.

#### 24. Payout math that over- or under-pays relative to the terms
**Verdict: defended.** The arithmetic is pinned at the balances, not at the
status: full happy path sums to exactly `total_amount`,
over-delivery caps at full contract value, partial delivery + grade
adjusts proportionally and refunds the shortfall to the buyer, and severe
shortfall never claws back an already-claimed advance. Tests:
`full_happy_path_both_tranches_claimed_pays_out_exactly_total_amount`,
`full_happy_path_advance_1_reclaimed_advance_2_claimed_still_sums_correctly`,
`over_delivery_does_not_pay_more_than_the_full_contract_value`,
`partial_delivery_pays_cooperative_proportionally_and_refunds_the_shortfall_to_buyer`,
`grade_adjustment_alone_reduces_the_cooperative_payout`,
`severe_shortfall_does_not_claw_back_advances_already_claimed`,
`lock_transfers_only_the_deposit_not_the_full_total`.

#### 25. On-chain allocation records leak personal data
**Verdict: defended by construction; the erasable half lives off chain.**
`member_hash` is `HMAC-SHA256(fresh per-member salt, phone_number)` computed
off chain — never a bare identifier — and the salt↔phone mapping lives only
in the API's Postgres: delete that row and the on-chain hash is permanently
unlinkable (NDPA s.34 erasure), because brute-forcing a random salt is
infeasible whatever the keyspace. The contract stores entries verbatim with
no normalization that would aid linkage. Tests:
`set_allocation_records_the_member_list`,
`set_allocation_with_duplicate_member_hashes_is_stored_verbatim`.
Residual accepted: the unlinkability property depends on the off-chain salt
scheme, which is the API's to keep — out of this contract's control.

### Keys, admin surface, and the token layer

#### 26. Admin, upgrade, pause, or owner takeover
**Verdict: defended by absence.** There is no admin key, no owner, no
pause, and no upgrade function — nothing exists to steal or extend, which
is the strongest form of PRD §4.5's "no unilateral drain, pause, or
redirect" and §16.3's "no upgrade key" answer. No test is possible or
needed for a capability that doesn't exist; the guard is that any PR adding
one violates authorization-matrix §4 and PRD §4.5 explicitly. Consequence
accepted in exchange: a deployed instance can't be patched in place — a
change means a new WASM and a new contract ID.

#### 27. One party's key is compromised
**Verdict: accepted risk, bounded by the actor table.** A stolen key can
only exercise the powers that role already has (the "cannot do" column
above), and no single role can move funds to an arbitrary destination or
settle unilaterally: the buyer can only reclaim their own vested money or
pay in more; the cooperative can only claim what the terms entitle it to;
the operator can attest but is never a payee. Worst realistic single-key
outcomes: a compromised cooperative claims advances it was entitled to
anyway and flags disputes (bounded pause); a compromised operator
attests falsely — which is entry 28, the central accepted trust. What the
contract cannot do is distinguish a stolen key from an honest one; key
custody is the wallet layer's job (PRD §4.5/§4.6).

#### 28. The warehouse operator attests dishonestly (or colludes with the
cooperative)
**Verdict: accepted risk — the central one, and it is the PRD's own
allocation of trust.** `confirm_delivery` trusts the operator's signature
as-is: no receipt hash, no second signature, no on-chain quantity evidence
beyond the caller's `require_auth()`. If the operator is wrong or bought,
delivery can be confirmed for goods that didn't arrive at grade — after the
buyer has funded the remainder — and `settle` will pay the cooperative.
This is accepted because (a) PRD §4.1 makes the operator the enforcement
backstop and routes grade disputes to the operator's own appeals process,
deliberately not here;(b) the blast radius is bounded: the buyer's defenses before the fact are the dispute freeze, the delivery deadline
(`reclaim_on_nondelivery`), and refusing to `fund_remainder` before
inspecting — with the caveat that the delivery-deadline defense is
defeasible, because `confirm_delivery` has no `delivery_deadline` guard, so
a late attestation can beat the buyer's reclaim, and the dispute freeze is a
tool the cooperative can use to create that window
([`dispute-frontrunning.md`](./dispute-frontrunning.md) §3.1); (c) the alternative — second-guessing the attestation on
chain — is the arbitration the PRD assigns elsewhere. Tests pinning what
the mechanism *does* cover:
`confirm_delivery_requires_warehouse_operator`,
`cannot_confirm_delivery_before_remainder_funded`,
`flag_dispute_freezes_every_other_state_changing_call`.

#### 29. Reentrancy through a hostile token's transfer hooks
**Verdict: accepted (hardened), not test-pinned.** State is written before
every external transfer (checks-effects-interactions) and Soroban's atomic
invocation unwinds storage writes on any panic, so a hook that called back
in would see committed state, not half-applied state. The current native/SAC
token has no such hook, so this closes no demonstrated exploit — it's
defensive ordering that costs nothing (HANDOFF design decisions). Its
correctness is a property of code ordering, reviewable but not asserted by
a dedicated test.

#### 30. Wrong token, wrong oracle address, absurd windows chosen at
`initialize`
**Verdict: accepted risk, layer-shifted by design.** The contract enforces
only mechanical bounds (`> 0` windows, bps ≤ 10_000); the API enforces a
1-hour floor and 90-day ceiling on `claim_window_secs` before it will build
an `initialize` transaction, but anything calling `initialize` directly
(CLI, tests, a future API) bypasses that. This was never a security
boundary — a buyer griefing themselves with a 1-second window harms only
their own deal. Tests pin the mechanical half:
`zero_claim_window_rejected_at_initialize`,
`zero_remainder_window_rejected_at_initialize`,
`zero_delivery_window_rejected_at_initialize`.

## Out of scope

| Attack / concern | Why it is explicitly out of scope |
| --- | --- |
| Arbitrating who was at fault (grade, quantity, "who breached") | The PRD assigns grade disputes to the warehouse operator's appeals process, not HarvestLock; this contract's dispute path only pauses. See [`dispute-scope.md`](./dispute-scope.md) §3 |
| Enforcing the off-chain reputation consequences (buyer's immediate bar, cooperative's 3-strike system) | Lives in the sibling `HarvestLock` repo's `api/` layer, where it is implemented (`api/src/reputation.ts`, `api/src/db/reputation.ts`); the on-chain half is only the `Defaulted`/`Forfeited` *distinction* it reacts to. See [`terminal-outcomes.md`](./terminal-outcomes.md) §7 for the exact trigger conditions and enforcement limits |
| The off-chain identity map, NDPA erasure execution | The salt↔phone rows are the API's Postgres, by design — this contract only stores the hash |
| Pro-rated on-chain payouts to member farmers (Transparency Ladder Rung 2+) | v1 is record-only per PRD §4.9's own stated default; adding payout logic would contradict it |
| On-chain "joint amendment" of a locked member list | No amend path exists on purpose (authorization-matrix §3.2); smuggling one in as a relaxed guard is forbidden |
| Key custody, social recovery, phishing, seed loss | Wallet layer (PRD §4.5/§4.6); no contract-side recovery mechanism exists to abuse |
| HarvestLock itself acting as a signer or arbitration key | No HarvestLock address exists in the contract; PRD open question 06 remains open, and answering it must not add a privileged entry point here |
| Compromising Reflector, wrong-but-fresh rates, Stellar consensus, Horizon, ledger reversion | Upstream of every oracle read and every ledger fact this contract consumes; it reads them, it doesn't secure them (entry 19 bounds the on-chain half) |
| Reflector not quoting NGN | Upstream feed gap, fail-closed here (entry 20), tracked in HANDOFF |
| Testnet resets; mainnet deployment | Testnet only, thrown away by design; nothing here is audited or deployable to mainnet yet — see README's Status section |
| API-layer availability, censorship, or mis-built transactions | Off chain; the contract can't stop the API from refusing to build a transaction, and doesn't need to |
| Confidentiality of amounts, parties, or terms | Nothing here is secret; a public ledger is the design, and no confidentiality is claimed |
| Front-running / MEV around permissionless calls | Not fully out of scope: the claim/reclaim boundary itself is pinned (`claim_exactly_at_deadline_still_succeeds`) and no third party can profit, but two ordering-dependent transfers exist and are analysed in [`dispute-frontrunning.md`](./dispute-frontrunning.md) — a post-deadline `confirm_delivery` beating `reclaim_on_nondelivery` (entry 28), and an expiry sweep beating a vested `reclaim_advance_*` (entry 21) |
| Buyer wiring a wrong token or self-harming parameters at `initialize` | Buyer-authenticated, buyer-borne; the API's floors are a business-judgment layer, not a boundary (entry 30) |

## Trust assumptions a reader should check

1. **The three parties' keys are their own** (KYB'd off chain, PRD's
   model). Every gate is only as strong as that.
2. **The warehouse operator is honest**, or its dishonesty is caught
   off chain (entry 28). This is the largest single trust in the system,
   and it is the PRD's choice, not an implementation accident.
3. **The buyer fixed the right terms at `initialize`** — token address,
   oracle address, windows, bps. There is no setter afterwards; the terms
   are the terms.
4. **The configured Reflector oracle is honest within the freshness
   bound** (entry 19), and `max_age_secs` is a window the parties would
   actually accept.
5. **Being named is not consenting**: the cooperative/operator never signed
   at creation, so a commitment naming them obliges them to nothing until
   they exercise their own auth'd calls.
6. **Off-chain enforcement exists**: the reputation bars are implemented in
   the sibling `HarvestLock` repo's `api/` layer and key off the contract's
   own `Defaulted`/`Forfeited` distinction; the appeals process and API
   floors are `api/`-side work, and the bar is enforced only when the API
   creates a new commitment (not on `reassign_buyer`, and not against
   anyone signing directly). See [`terminal-outcomes.md`](./terminal-outcomes.md) §7.
7. **Observability works**: `status_changed` events and the view functions
   are how anyone detects a transition; `every_transition_publishes_status_changed`
   holds that every mutation is announced (and `initialize` deliberately
   announces nothing — creation is not a transition).

## What this document does not authorize

The contract is **not audited and not deployed to mainnet** — the Status
section in [`README.md`](../README.md) is the record of what has to happen
before that changes. Nothing here makes testnet deployments suitable for
real money: every address referenced in HANDOFF.md is a throwaway
validation artifact. Entries 27 and 28 in particular (key compromise,
operator dishonesty) are accepted, not solved; a reader planning a real
pilot should treat them as due-diligence items for the counterparty
selection process, not as guarantees this code provides.

## Maintaining this document

- A **new attack class** gets an entry with a verdict, and if the verdict
  is "defended" it gets a test — the entries above are why most of the
  guard-shaped tests in `test.rs` exist under the names they do.
- A verdict that changes from "accepted" to "defended" is a code change,
  not a documentation change: update the entry and cite the test that now
  holds it.
- A verdict that changes from "defended" to "accepted" is a regression, to
  be argued explicitly in the diff with the reason — the same discipline
  HANDOFF.md applies to its own "deliberately NOT implemented" list.
- If a new public function changes the authorization surface, update
  [`authorization-matrix.md`](./authorization-matrix.md) §1 in the same PR
  (CI's `check_auth_docs.sh` enforces the doc-comment half); if it
  introduces a new fund-moving or deadline path, this file's attack classes
  need an entry too.
