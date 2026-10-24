# Authorization matrix — every `require_auth()` in the escrow contract, cross-checked against the PRD

Issue #66 asked for a complete pass: list every `require_auth()` call in
`contracts/escrow/src/lib.rs`, name the party each one requires, and hold
the result against the PRD's authorization statements. This doc is that
pass. It also names the two places where the as-built auth is
*deliberately stricter* than a plain reading of the PRD, so nobody
"corrects" them back.

Ground truth is the code (`contracts/escrow/src/lib.rs`); the spec is the
PRD ([`docs/PRD.md`](https://github.com/Agrisettle/HarvestLock/blob/main/docs/PRD.md)
in the sibling `HarvestLock` repo). Where this doc and the PRD disagree
about intent, the PRD wins; where they disagree about what the contract
*does*, the code wins. The dispute trio (`flag_dispute` /
`resolve_dispute` / `expire_dispute_window`) has its own deeper doc —
[`dispute-scope.md`](./dispute-scope.md) — this one only places it in the
full matrix.

## TL;DR

- 30 public functions, 5 authorization shapes, **zero un-gated
  fund-moving paths**: 13 single-party gates, 8 permissionless state
  changes, 3 multi-party mutual actions, 1 caller-authenticated flag,
  5 no-auth views.
- **Permissionless is deliberate, and safe by construction** — a
  permissionless call can only complete a check the deal already fixed at
  lock-in (a deadline lapsed, both tranches resolved, FX comparison
  computed). It can never grant anyone anything they weren't already
  entitled to under the agreed terms, and never redirects anything.
- **No admin surface exists at all**: no owner, no pause, no upgrade, no
  privileged entry point. That is the contract's strongest match to PRD
  §4.5's "no unilateral drain, pause, or redirect" claim.
- Two deliberate divergences from a plain PRD reading — both *stricter*
  than the PRD, both documented below: `reassign_buyer` requires a third
  signature (the incoming buyer's) beyond §4.8's "cooperative consent";
  and the PRD's member-list "joint amendment" path does not exist on
  chain (the ledger is immutable, as §4.9's record-only design intends).

## 1. The complete matrix

`require_once_each` = one `require_auth()` per *distinct* address
(same-address parties don't double-sign). "Permissionless" = the function
calls no `require_auth()` at all.

| Function | Auth required | PRD basis |
| --- | --- | --- |
| `initialize` | **Buyer only** | §4.5 — the buyer fixes the deal's release conditions at creation; cooperative/operator co-signing creation is not asked for. |
| `set_allocation` | **Cooperative** | §4.9 Rung 1 — the cooperative authors the member entitlement record. Cooperative-gated, not buyer-consented (it is the cooperative's own membership data). |
| `lock` | **Buyer** | §8 — deposit of the advance tranches happens "on lock-in". |
| `release_advance_1` | **Permissionless** | §8 tranche 1 — merely opens the claim window; can't move funds. |
| `claim_advance_1` | **Cooperative** | §8 — the tranche-1 entitlement holder claims it. |
| `reclaim_advance_1` | **Buyer** | §8 — "expiring back to the buyer if unclaimed within N days". |
| `mark_checkpoint` | **Warehouse operator** | §8 tranche 2 — "on mid-season checkpoint attestation". |
| `release_advance_2` | **Permissionless** | §8 tranche 2 — opens the claim window; can't move funds. |
| `claim_advance_2` | **Cooperative** | §8 — same as tranche 1. |
| `reclaim_advance_2` | **Buyer** | §8 — same as tranche 1. |
| `ready_for_delivery` | **Cooperative** | Signals intent to deliver; opens the remainder-payment window. |
| `fund_remainder` | **Buyer** | The second half of two-phase funding (Dep. 5) — buyer pays the balance of the escrow. |
| `expire_remainder_window` | **Permissionless** | Buyer-default consequence: the deadline either passed unfunded or it didn't — who calls changes nothing. Sweeps to the cooperative per the terms fixed at lock-in. |
| `reclaim_on_nondelivery` | **Buyer** | Seller-non-delivery: returns the current balance to the buyer once the delivery deadline passes unfulfilled. Pays the caller's own reclaim right, so gated on the caller. |
| `confirm_delivery` | **Warehouse operator** | §4.1/§4.5 — the operator is the independent delivery oracle; "release requires buyer plus warehouse-operator attestation". |
| `settle` | **Permissionless** | §4.1 — conditional release executes pre-agreed math on already-recorded state; nobody (not even the buyer) can withhold it once conditions are met. This is what "no unilateral drain" actually buys. |
| `resolve_fx_shortfall` | **Permissionless** | §4.2 option (b)/§16.3 — a one-time comparison of escrowed-vs-converted obligation; deterministic on the oracle read. |
| `refresh_fx_quote_status` | **Permissionless** | #1 — records the answer of the same staleness check (`Commitment::fx_quote_stale`, the "waiting on a fresh quote" signal). Informational only: takes no outcome parameter, moves nothing, and no guard reads the flag back. |
| `fund_fx_shortfall` | **Buyer** | §4.2 option (b) — "buyer tops up" at settlement when the rate moved. Fund-mover, correctly gated to the payer. |
| `expire_fx_shortfall_window` | **Permissionless** | Buyer-default path if the top-up never funds; same uncontested deadline logic as `expire_remainder_window`. |
| `cancel` | **Buyer + cooperative** | Edge table — "Mutual cancellation: defined unwind… no penalty, logged". Mutual by definition, both signatures required. |
| `reassign_buyer` | **Current buyer + cooperative + incoming buyer** | §4.8 — "transferable to another KYB-verified buyer with cooperative consent". The *incoming buyer's* signature is deliberately beyond the PRD (see §3.1). |
| `flag_dispute` | **The flagger**, who must be one of the three named parties (own auth; strangers rejected `NotAParty`) | §4.8 must-have "dispute flagging with defined escalation". See `dispute-scope.md`. |
| `resolve_dispute` | **All three parties** | Unanimous resume — `dispute-scope.md` §2. |
| `expire_dispute_window` | **Permissionless** | Timeout resume so one refuser can't freeze a commitment forever. |
| `get_oracle_config`, `oracle_rate`, `get_allocation`, `get_status`, `get_commitment` | **None (views)** | Read-only; no state change, no funds. |

Auth internals worth knowing when reading the code: `claim_advance_1/2`
route through the private `claim_tranche` (cooperative auth) and
`reclaim_advance_1/2` through `reclaim_tranche` (buyer auth), so the
matrix reads the site of enforcement, not the public wrapper.
`require_once_each` deduplicates — if two named roles share one address,
that address signs once (tests: `cancel_succeeds_when_buyer_and_cooperative_share_one_address`,
`reassign_buyer_succeeds_when_buyer_and_cooperative_share_one_address`,
`all_three_identical_parties_accept_only_that_principals_signature`,
`two_of_three_identical_parties_still_require_the_distinct_partys_auth`).

## 2. Why "permissionless" is safe here, not sloppy

The eight permissionless functions share one property, and it is worth
stating as an invariant a reviewer can check each against: **the call can
only complete a fact the deal already fixed at lock-in.**

- `release_advance_1/2` move nothing — they timestamp a claim window
  that the state machine already defined. (Security-reviewed on the
  record in [permissionless-release-review.md](./permissionless-release-review.md),
  #2, with the adversarial-timing scenarios pinned by
  `tests/permissionless_release_review.rs`.)
- `expire_remainder_window` / `expire_fx_shortfall_window` execute a
  deadline the buyer's own funding behavior decided. Waiting for the
  *other* side to call the buyer-default path would hand the defaulting
  party a veto over the consequence of its own default.
- `settle` pays recorded, already-determined amounts (settlement_bps,
  already-claimed advances, funded shortfall) — the math is closed
  before the call.
- `resolve_fx_shortfall` is one-time and deterministic on the oracle
  read; re-resolving isn't allowed (`FxAlreadyResolved`), so timing of
  the call buys nobody anything.
- `refresh_fx_quote_status` merely records what a fresh oracle read
  said (the #1 "waiting on a fresh quote" flag); no outcome parameter,
  no guard reads the flag, and the worst a wrong flag can do is mislead
  a display.
- `expire_dispute_window` restores exactly the pre-dispute status — it
  cannot decide anything the parties hadn't already agreed.

None of them accepts an outcome parameter from the caller. That is the
line between this and the "privileged entry point" the non-custodial
claim would forbid — and it is the same line PRD §4.5 draws ("no
unilateral drain, pause, or redirect").

## 3. Drift against the PRD — deliberate, and documented

### 3.1 `reassign_buyer`: three signers, where §4.8 says one consent

PRD §4.8: the buyer position is "transferable to another KYB-verified
buyer with cooperative consent". The contract requires **three**
signatures: current buyer, cooperative, *and the incoming buyer*. This is
stricter than the PRD, deliberately (HANDOFF Deployment 4): novation
forced onto an unwilling incoming buyer is not a transfer anyone should
want, and the incoming buyer is the one party the PRD text leaves
unmentioned precisely because transfer mechanics were underspecified
there. The contract took the safe superset. **Do not "fix" this to two
signatures without a product decision that an unwilling buyer can be
assigned a live obligations contract.**

### 3.2 Member-list disputes: no on-chain "joint amendment"

The PRD's edge-case table says allocation is "immutable post-lock-in
except by joint amendment; disputes resolve off-chain against the
recorded list." The contract implements the immutable half — no amend
function exists — and the off-chain-resolve-against-the-recorded-list
half is a process matter that needs no contract surface. The "except by
joint amendment" clause has no on-chain path today. This is consistent
with §4.9 Rung 1 (record-only ledger), but if a pilot ever needs an
on-chain amendment, it is new work with real design questions (what
counts as joint — buyer too? — and how to audit the change); it must not
be smuggled in as a relaxed guard on `set_allocation`.

### 3.3 Where the PRD and code agree, verbatim

- §4.5 "HarvestLock holds no keys… is not a signer" — there is no
  HarvestLock address, admin key, or pause anywhere in the contract.
- §16.3's least-bad answer for upgradeability — "no upgrade key" — is the
  as-built state in its strongest form: no upgrade function was ever
  written.
- §4.6 "contract terms bind the cooperative entity, not the lead's
  personal key" — the contract's only notion of a party is an `Address`;
  the social-recovery machinery lives at the wallet layer by design.
- §4.5 "release requires buyer plus warehouse-operator attestation" — as
  built: buyer funds (`lock` / `fund_remainder` / `fund_fx_shortfall`),
  operator attests (`confirm_delivery`), release (`settle`) then runs
  permissionlessly. The buyer's authorization is exercised at funding
  time, not as a per-payout veto — once the conditions the buyer set at
  creation are met, no one can unilaterally hold the money back. That
  inversion is the point of the non-custodial design, and it is the one
  place a reader should double-check their intuition.

## 4. Guardrails for future contributors

- **Every new public function must pick a shape from §1** and say so in
  its doc comment — CI enforces the comment half
  (`.github/scripts/check_auth_docs.sh`: auth vocabulary required on
  every public method), and this file is the human-readable other half.
- **Multi-party functions compose through `require_once_each`**, never
  chained raw `require_auth()` calls — the dedupe and replay-safety
  properties are test-pinned (`auth_entry_for_one_method_cannot_authorize_a_different_method`,
  `auth_entry_for_one_argument_set_cannot_authorize_a_different_argument_set`,
  `expired_staged_auth_entry_fails_cleanly_with_no_partial_state`).
- **A permissionless function may not take an outcome parameter.** If a
  proposed function needs one, it is a party-gated action wearing the
  wrong clothes.
- **No privileged role may be added without revisiting §4.5.** There is
  currently no admin surface to extend; the regulatory posture in the
  PRD depends on that staying true.
