# Dispute scope — what `flag_dispute` / `resolve_dispute` cover, and what they deliberately don't

A reader meeting this contract for the first time could reasonably assume
"dispute resolution" means the contract decides who was right. It doesn't.
This doc states the boundary plainly, so nobody builds on — or integrates
against — arbitration coverage that was never intended.

Ground truth is the code (`contracts/escrow/src/lib.rs`, `flag_dispute` /
`resolve_dispute` / `expire_dispute_window` doc comments) and
[`HANDOFF.md`](../HANDOFF.md) Deployment 10. The spec is the PRD
([`docs/PRD.md`](https://github.com/Agrisettle/HarvestLock/blob/main/docs/PRD.md)
in the sibling `HarvestLock` repo). Where this doc and the PRD disagree about
intent, the PRD wins; where they disagree about what the contract *does*, the
code wins.

## TL;DR

- The PRD assigns **grade/quantity disputes to the warehouse operator's own
  appeals process**, explicitly *not* a HarvestLock dispute. The contract
  follows that: `confirm_delivery` trusts the operator's attestation as-is.
- What the contract *does* implement — the PRD's must-have "dispute flagging
  with defined escalation" — is a **bounded, unilateral pause**, not
  arbitration: any one named party freezes the commitment; unanimous consent
  or a timeout resumes it at exactly the pre-dispute status. Nobody's fault is
  decided, no payout is redirected, no outcome is rewritten.
- The uncontested failure cases (buyer default, seller non-delivery, mutual
  unwind) never needed the dispute path and still don't — they are
  deadline-triggered or mutually-agreed, not contested-fault claims.

## 1. What the PRD actually asks for

The PRD's dispute framing is distributed across several sections, and it is
consistent once assembled. The key move happened in v0.3: v0.2 made
HarvestLock itself responsible for delivery verification, grading, and
dispute arbitration; v0.3 (PRD §4.1) moved all three to the warehouse
operator as keystone partner, calling v0.2's assumption "a field-operations
business a small team cannot run."

Concretely, the PRD says:

- **Grade disputes belong to the operator (§4.1 table, edge-case table).**
  "Who adjudicates grade disputes? The operator's existing grading process
  and appeals." The edge-case row for grade mismatch reads: "Operator's
  grading is authoritative; pre-agreed grade-price table; operator's appeals
  process, not a HarvestLock dispute."
- **Member-list disputes resolve off-chain.** The edge-case row for a
  disputed member list: "Allocation immutable post-lock-in except by joint
  amendment; disputes resolve off-chain against the recorded list."
- **Arbitration is separated from custody (§4.5).** "Where a human tiebreak
  is unavoidable, the arbitration key sits with a party that is not
  HarvestLock." The same section is honest about staging: a v1 pilot may have
  no credible alternative to HarvestLock holding a tiebreak key, which if so
  must be "a temporary, disclosed position with a written path to handing it
  over — not a permanent design." Open question 06 ("Who holds the
  arbitration key if not HarvestLock, from day one?") is still open.
- **The feature list asks for flagging + escalation, not a verdict.**
  The v1 must-have is literally "Dispute flagging with defined escalation"
  — a reporting and routing mechanism. The pilot success metric is the
  "share of contracts settling without escalation beyond the operator's
  grading process," which again locates substantive escalation *at the
  operator*, not in HarvestLock.
- **The §4.8 state machine shows `Disputed` as a branch off the main flow**
  (`Locked → … → Delivered`, with `Cancelled` / `Defaulted` / `Disputed`
  below), with no on-chain resolve arrow drawn. The contract fills that gap
  with a resume-to-pre-status semantic (below) — a choice the PRD neither
  forbids nor specifies.

So the PRD's "original arbitration framing" is, accurately read, an
*anti*-arbitration framing for HarvestLock itself: push grade truth to the
operator, keep HarvestLock out of custody (§4.5) and out of the tiebreak,
and require only that a dispute can be flagged and escalated somewhere
defined.

## 2. What the contract implements

Three functions, Deployment 10 (`HANDOFF.md`), 17 unit tests, live-verified
on testnet:

| Function | Who can call | What it does |
| --- | --- | --- |
| `flag_dispute(flagger)` | Any **one** of buyer, cooperative, `warehouse_operator` (own auth required; strangers rejected with `NotAParty`) | Records current status in `dispute_pre_status`, sets `dispute_deadline = now + remainder_window_secs`, moves status to `Disputed`. Reachable from `Locked` through `Delivered` only — not `Draft` (nothing escrowed to freeze) and not any terminal status or `Disputed` itself. |
| `resolve_dispute()` | **All three** named parties, same call (`require_once_each` — once per distinct address, so overlapping roles don't double-auth) | Restores the exact pre-dispute status, clears `dispute_pre_status` / `dispute_deadline`. Decides nothing and redirects nowhere — see below. |
| `expire_dispute_window()` | **Anyone** (deliberately permissionless), once `dispute_deadline` has passed | Same restore as `resolve_dispute`. Exists so one party refusing to consent can't freeze the commitment forever. |

The freeze works by construction, not by a blocklist: every other
*status-gated* function matches a specific required status (or set of
statuses), and `Disputed` is never among them, so while a commitment is
`Disputed` none of them can move it. One deliberate exception is worth
stating because "nothing else can move" would be literally false:
`claim_advance_1/2` and `reclaim_advance_1/2` never check `status` —
`claim_tranche` / `reclaim_tranche` guard only the tranche deadline, the
one-shot flags, and the party's auth. A dispute therefore does **not**
block a claim or a reclaim, and cannot be used to protect a tranche from
the other side. See [`dispute-frontrunning.md`](./dispute-frontrunning.md)
§2 for why that matters. `resolve` / `expire` restore
*exactly* the status recorded at flag time — if the off-chain resolution is
"unwind the deal," the parties call the existing `cancel()` next, same as
they would have without ever disputing.

One known limitation is documented in `flag_dispute`'s doc comment and
pinned by a dedicated test rather than left as prose: **the freeze does not
pause any other absolute deadline already ticking** (advance claim
deadlines, `remainder_deadline`, `delivery_deadline`,
`fx_shortfall_deadline`). A deadline that lapses mid-dispute becomes
actionable the moment the dispute clears. Shifting every deadline by the
dispute duration would close this, but was left open deliberately as
unneeded complexity for a mechanism nobody has used yet.

## 3. Covered vs. deliberately not covered

| Situation | Covered by the dispute path? | Where it actually lives |
| --- | --- | --- |
| A party believes something is wrong and needs the state machine to stop moving while it's sorted out off-chain | **Yes — this is the whole of it.** Unilateral freeze, bounded by unanimous-or-timeout resume. | `flag_dispute` / `resolve_dispute` / `expire_dispute_window` |
| Grade or quantity is contested (operator said Grade B / 500 units, a party says otherwise) | **No.** Operator's grading is authoritative; its appeals process decides. | `confirm_delivery` doc comment; PRD edge-case table. The contract records what the operator attested (`delivered_quantity`, `grade_index` → `settlement_bps`) with no receipt-hash or second-opinion artifact beyond the operator's own `require_auth()`. |
| Deciding who was at fault, or redirecting the commitment to a different outcome (e.g. straight to `Cancelled`) from inside the dispute call | **No — explicit non-goal.** Both resume paths restore the pre-dispute status verbatim. | `resolve_dispute` / `expire_dispute_window` doc comments: doing otherwise "would be arbitration by another name." |
| Buyer never funds the remainder (buyer default) | **No dispute needed.** Uncontested deadline check, permissionless, pays the cooperative, sets `Defaulted` (immediate permanent bar off-chain). | `expire_remainder_window` |
| Cooperative never delivers (seller non-delivery) | **No dispute needed.** Uncontested deadline check, buyer-gated (it pays the caller's own reclaim right), sets `Forfeited` — deliberately distinct from `Defaulted`. | `reclaim_on_nondelivery` |
| Both sides agree to unwind | **No dispute needed.** Mutual, not contested. | `cancel` (buyer + cooperative auth) |
| Member allocation is contested | **No.** Immutable post-`lock` except by joint amendment; resolved off-chain against the recorded list. The ledger is record-only in v1 anyway (`settle` still pays a lump sum to the cooperative wallet, PRD §4.9 Rung 1). | `set_allocation` doc comment; PRD edge-case table |
| A trusted third party breaks a tie on-chain | **No such role exists in the contract.** There is no arbitration key, no HarvestLock signer, no tiebreak entry point — matching PRD §4.5's separation, and leaving PRD open question 06 open. | Absence is the design; see §4 below. |

## 4. Notes for future contributors and integrators

- **Don't route grade appeals through this contract.** A future
  "grade-dispute" feature that re-litigates `confirm_delivery`'s inputs
  on-chain contradicts the PRD's warehouse-anchored design (§4.1) and the
  edge-case table. The correct integration point for a contested grade is
  the operator's appeals process, off-chain; the contract re-enters only via
  the existing resume paths.
- **Don't "improve" `resolve_dispute` into a verdict.** Accepting a
  fault finding, a payout split, or a redirect-to-`Cancelled` parameter
  would be the arbitration the PRD assigns elsewhere. The resume-then-`cancel`
  two-step is the intended composition — keep it.
- **Don't gate the forfeiture paths on a dispute.** `expire_remainder_window`
  and `reclaim_on_nondelivery` were built earlier and separately precisely
  because uncontested, deadline-triggered outcomes must not wait on a
  mechanism designed for contested ones (`HANDOFF.md`, "What's deliberately
  NOT implemented" item 1). A dispute freeze blocks them like everything
  else while open, but neither requires a dispute to fire.
- **Don't assume the freeze stops the clock.** Any integration that tells a
  user "your position is safe while disputed" must account for the known
  limitation in §2: other deadlines keep ticking. If a real dispute ever
  collides with one, revisit deadline-shifting then — not speculatively now.
- **A dispute is a delay, not a free option.** `flag_dispute` costs one
  transaction, carries no bond or reputation event, and can be repeated
  immediately after `expire_dispute_window`. The full incentive analysis
  is in [`dispute-stalling.md`](./dispute-stalling.md); the races it
  participates in are in
  [`dispute-frontrunning.md`](./dispute-frontrunning.md).
- **The arbitration-key question is still open, and still not this
  contract's to answer.** If PRD question 06 is ever answered with "party X
  holds a tiebreak key," that key belongs in custody/off-chain procedure
  (PRD §4.5), not as a new privileged entry point here — adding one would
  violate the "no unilateral drain, pause, or redirect" non-custodial
  property the regulatory posture depends on.
