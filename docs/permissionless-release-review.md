# Security review: are `release_advance_1`/`release_advance_2` safely permissionless?

**Issue**: [Agrisettle/HarvestLock-Contracts#2](https://github.com/Agrisettle/HarvestLock-Contracts/issues/2).
**Scope**: the two public entry points and the state they touch —
`release_advance_1`/`release_advance_2` and their shared implementation
`open_tranche` (`contracts/escrow/src/lib.rs`), audited against every other
function that reads the fields they write (`advance1_deadline`,
`advance2_deadline`, and the status transitions they enable).
**Verdict**: **the permissionless design holds. No auth gate is added.** The
doc comments on both functions already argued this defensively ("this happens
to be safe"); this review stress-tested that argument against the adversarial
scenarios issue #2 named, answered each on the record, and pinned the answers
with a regression suite (`contracts/escrow/tests/permissionless_release_review.rs`)
so the reasoning can't quietly go stale. Recorded confirmation, per issue #2's
own framing ("This might conclude *yes, correct as-is*… that's a legitimate
outcome").

This document is the "written response" issue #2 asked for. It sits alongside
the two documents that already made the general argument —
[authorization-matrix.md](./authorization-matrix.md) §2 ("why *permissionless*
is safe here, not sloppy") and [threat-model.md](./threat-model.md) entry 21
("a third party seizes the timing of a permissionless first call") — and goes
one level deeper on *these two functions specifically*, because HANDOFF.md's
"deliberately NOT implemented" list asked for exactly this pair to get a
reviewer's second look on the record.

## What these calls actually do

`open_tranche` (shared by both entry points) is the whole implementation. In
one invocation it: (1) loads the commitment, (2) checks `status` equals the
exact required status (`Locked` for tranche 1, `CheckpointPassed` for tranche
2), (3) writes `deadline = now + claim_window_secs` to the tranche's deadline
field, (4) advances `status`, (5) saves. **No token client is ever
constructed; no transfer happens; no caller-controlled argument exists at
all** — both entry points take `env` and nothing else. Test-pinned:
`opening_a_tranche_moves_no_funds_for_either_tranche`.

Everything downstream of the new status is either auth-gated
(`claim_*` — cooperative, `reclaim_*` — buyer, `mark_checkpoint` — warehouse
operator) or itself deadline-driven (`settle`'s tranche-resolution guard reads
the `claimed`/`expired` *flags*, never the deadlines).

## Question 1 — can a third party grief by starting the clock early?

**No meaningful grief is reachable; the worst case is bounded by the terms
the parties themselves fixed at `initialize`.**

What an attacker actually controls by calling either function: *the instant
the claim window opens*, and therefore the instant it closes
(`deadline = open time + claim_window_secs`). Nothing else. Walking issue #2's
scenario directly:

1. **"Start the claim window at an inconvenient moment, forcing the
   cooperative into the reclaim path prematurely."** There is no
   "premature" reclaim path. `reclaim_*` becomes eligible at exactly one
   fact — the window passed unclaimed — and that fact is identical no matter
   *who* started the clock. The cooperative's protection against lapsing is
   the window length it agreed to at `initialize` (one signature, one call to
   claim) plus the fact that it can pre-sign: nothing about claiming requires
   the cooperative to be online at any particular second, only before the
   deadline. A cooperative that cannot execute a single pre-authorized
   transaction for the entire `claim_window_secs` has an operational problem
   the contract cannot fix, and the API layer additionally enforces a 1-hour
   floor / 90-day ceiling on `claim_window_secs` before it will even build an
   `initialize` transaction (HANDOFF.md, "deliberately NOT implemented",
   item 5) — so a pathologically short window is not a deployment shape this
   product ships.

2. **Who loses what, and does the attacker gain?** If the window lapses
   unclaimed, the deposit tranche goes back to the **buyer**
   (`reclaim_*`, buyer-auth-gated) — not to the caller, not to the
   cooperative, not to anyone the attacker can influence. The attacker pays
   a transaction fee and captures nothing: no funds move at release, no
   function pays or empowers the caller, and the only party the early clock
   could cost is the counterparty whose *own agreed terms* define the window.
   This is the threat-model entry-21 shape exactly: a permissionless call
   can only complete a fact the deal already fixed; the timing it picks is
   worth nothing to anyone.

3. **Tranche 2 cannot even be raced from lock-in.** `release_advance_2`
   requires `CheckpointPassed`, which only the warehouse operator's
   auth-gated `mark_checkpoint` can produce. A third party cannot pull the
   tranche-2 clock forward past a checkpoint that hasn't happened — the
   earliest tranche 2 can open is "the instant after the operator attested,
   whoever calls next."
   Test-pinned: `no_clock_can_be_started_during_a_dispute_freeze`,
   `out_of_sequence_states_refuse_the_clock`.

4. **Dispute freeze interaction.** While a dispute is open, `status` is
   `Disputed`, which is never a required status — so *nothing*, party or
   stranger, can start a clock during a freeze. The pre-existing known
   limitation `flag_dispute`'s doc comment records (absolute deadlines don't
   pause during a freeze) applies identically whether a clock was opened by
   a party or a stranger; it is not made worse here, and it is already
   documented as accepted, revisitable design debt.

5. **No restart, no extension, no re-opening.** After the first call the
   status has moved, so a second `release_*` fails `InvalidState`, and
   `set_tranche_deadline` is reachable from exactly one place (`open_tranche`)
   behind that exact-status guard. The deadline is written once and can never
   be rewritten. Test-pinned: `the_clock_cannot_be_restarted_or_extended_once_open`.

## Question 2 — does "it doesn't move money" hold under every state it can be called from?

**Yes. Audited field-by-field, not by re-reading the doc comment.**

Complete reader audit of the two fields `open_tranche` writes:

| Field | Every reader | Can an early write change its effect? |
|---|---|---|
| `advance1_deadline` / `advance2_deadline` | `claim_tranche` (`timestamp > deadline` rejects) | No — the claim guard is a pure function of (timestamp, deadline); opening earlier only moves the deadline earlier, which is precisely the intended semantics of "the window started earlier" |
| same | `reclaim_tranche` (`timestamp <= deadline` rejects) | No — exact complement of the above; the buyer's reclaim right is *created*, never transferred, and remains buyer-auth-gated |
| (status) | every other state-changing function | No — each requires an exact status or set; the two new statuses appear only as targets here and as requirements of the auth-gated `claim_*`/`mark_checkpoint`/`ready_for_delivery` and the buyer-gated `reclaim_*` |

Crucially, **nothing downstream infers an outcome from the deadline's
*provenance***: `settle` and `resolve_fx_shortfall` gate on the
`advance*_claimed`/`advance*_expired` flags (which only the auth-gated
`claim_*`/`reclaim_*` can set), and every other function gates on `status`
alone. There is no code path anywhere that asks "was this deadline set at a
natural time" — because there is no such notion in the state machine; the
deadline is the sole input to claim/reclaim eligibility, and who wrote it is
not recorded or trusted.

Money-movement audit: `open_tranche` constructs no token client. The three
transfer sites reachable from the new statuses are `claim_tranche` (pays the
cooperative — cooperative auth), `reclaim_tranche` (pays the buyer — buyer
auth), and later `settle` (pays recorded amounts, permissionless by the same
principle this review examined). No transfer's amount, recipient, or
eligibility depends on who opened the window.
Test-pinned: `opening_a_tranche_moves_no_funds_for_either_tranche`,
`a_stranger_cannot_claim_a_window_it_opened`,
`a_stranger_cannot_reclaim_a_window_it_opened`,
`the_full_griefing_scenario_pays_only_the_entitled_parties`.

## Question 3 — is there a *positive* reason for permissionless, beyond "harmless"?

**Yes, three, and they should be stated as design rationale rather than left
as a defensive fallback:**

1. **Automation without custody.** A watcher bot can drive the state machine
   forward with no party's wallet online — the same deliberate pattern
   `expire_remainder_window`, `expire_fx_shortfall_window`,
   `expire_dispute_window`, `resolve_fx_shortfall`, and `settle` already
   use. Each of those is permissionless for the same recorded reason: the
   call completes a pre-fixed fact, so gating it on a signature would only
   add a liveness dependency, never safety.
2. **Auth-gating would create a timing veto that doesn't exist today.**
   Whoever holds the hypothetical gate key gains the power to choose *when*
   the counterparty's claim clock starts. If that key were the buyer's, the
   buyer could shop for an inconvenient moment — exactly the grief this
   question worries about, except handed to a *party with a stake* and with
   the added power to simply never call at all (a stall, which today's design
   cannot do). PRD §4.5's "no unilateral drain, pause, or redirect" argues
   against manufacturing that lever.
3. **Liveness of a two-phase escrow.** Tranche windows open on eligibility,
   not on someone's availability. The party who *would* call it in the honest
   case (the cooperative, to start its own claim window) loses nothing by a
   stranger calling first — it can claim in the same second — and the party
   who might benefit from delay (nobody, per question 1) has no path to it.

## What would change the verdict

Two things, both product decisions, neither an auth gate:

- A deployment that wants a *minimum* time before tranche 1 may open (so the
  window can't start until, say, 24h after lock-in) needs an
  `earliest_release_1`-style term fixed at `initialize` — a new *condition of
  eligibility*, visible to both parties up front, not a signature gate whose
  holder picks the moment.
- Evidence that a real cooperative deployment actually misses claim windows
  it could have met. Nothing in the ten testnet deployments (HANDOFF.md)
  shows that; revisit then.

## What this review produced

- This document — the on-record answer issue #2 asked for.
- `contracts/escrow/tests/permissionless_release_review.rs` — the regression
  suite pinning every claim above (permissionless under a stranger-only auth
  set, no funds moved, no restart, full-window guarantee, dispute-freeze
  interaction, and the end-to-end griefing scenario paying only the entitled
  parties), with its own CI job.
- Tightened doc comments on `release_advance_1`/`release_advance_2` stating
  the conclusion and pointing here, instead of the previous defensive
  one-liner.
- HANDOFF.md's "deliberately NOT implemented" item 6 updated from "still
  worth a security reviewer's attention on sight" to point at this record.

**Conclusion: correct as-is.** The permissionless release calls are safe by
construction — not because a reviewer read the comment and agreed, but
because every state they write, every reader of that state, and every
adversarial timing scenario issue #2 named is now pinned by a test that will
fail if any of it ever changes.
