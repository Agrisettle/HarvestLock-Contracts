# Front-running and race exposure on dispute flagging, resolution, and the deadline paths

Issue #117 asked whether a pending `flag_dispute` or `resolve_dispute` is
visible before it lands, and whether any party gains a real advantage by
observing and reacting to it — for example, racing to claim a tranche
before a dispute freeze lands. The answer needs one important correction
up front: **the freeze does not stop tranche claims** (§2), so that
particular race does not exist. But four other races do, and two of them
move the entire remaining escrow.

Ground truth is `contracts/escrow/src/lib.rs`. Companion docs:
[`dispute-stalling.md`](./dispute-stalling.md) (why a party would flag at
all), [`terminal-outcomes.md`](./terminal-outcomes.md) (what each outcome
pays), and [`griefing-and-inaction.md`](./griefing-and-inaction.md)
(inaction paths). Platform facts below are about Stellar/Soroban, not
EVM, and are stated conservatively.

## TL;DR

- There is **no public EVM-style mempool** on Stellar, so an unrelated
  searcher cannot watch a pending transaction and sandwich it. The
  exposure here is **counterparty last-look**, not third-party MEV: the
  parties already know the deal's deadlines and next steps, and the
  API's staged multi-signature proposals make a pending call visible to
  its co-signers by design.
- **No third party can profit from any race in this contract.** Every
  payout is hard-wired to `buyer` or `cooperative`; permissionless
  callers choose *when*, never *to whom*, and there is no caller fee.
  The races are zero-sum between the two economic parties.
- **The headline finding is a post-deadline race between
  `confirm_delivery` and `reclaim_on_nondelivery`.** `confirm_delivery`
  has no `delivery_deadline` guard, so after the deadline both calls are
  legal; whichever lands first decides whether the cooperative keeps the
  remainder or the buyer gets it back. `flag_dispute` is the tool that
  lets the cooperative win that race by blocking the buyer for a full
  window.
- **The second finding is the `expire_remainder_window` sweep eating a
  lapsed, unreclaimed tranche.** The buyer has a vested `reclaim_*`
  right; the sweep transfers it to the cooperative. This is not
  theoretically order-dependent — it is *only* order-dependent, and both
  transactions can succeed in the same ledger, with different amounts.
- **`resolve_fx_shortfall` is a one-shot, permissionless, first-caller-
  fixes-the-number oracle read** with no commitment scheme. The two
  parties have opposite incentives on the rate, so its timing is a
  genuine race.
- **`flag_dispute` versus any status-gated call** is a one-block race
  whose prize is a window of delay; repeated flags make the delay
  unbounded. This is the bridge to issue #118.
- The rest of the surface is **order-independent by construction**:
  `settle`, `release_advance_*`, `cancel` (two signatures), and
  `reassign_buyer` (three signatures) cannot be usefully raced.

## 1. What "visible before it lands" means on Soroban

Three channels matter, and none of them is a public mempool:

1. **The workflow itself.** The party who benefits from a permissionless
   call already knows the deadline and the state. Anyone can compute the
   exact ledger timestamp at which `expire_remainder_window`,
   `expire_dispute_window`, `expire_fx_shortfall_window`, or
   `reclaim_on_nondelivery` becomes callable. No observation of a pending
   transaction is needed to prepare a competing one.
2. **Staged multi-party proposals in the API.** `reassign_buyer`,
   `cancel`, and `resolve_dispute` require several signers, so the API
   stages a proposal for co-signing (`api/src/db/pendingMultisigProposals.ts`,
   driven by `api/src/stellar/multiParty.ts`). That proposal is *explicitly visible
   to the counterparties* — it has to be, or they could not co-sign. A
   counterparty therefore sees a pending multi-party call before it is
   submitted, which is a stronger leak than a mempool.
3. **The RPC provider and the ledger.** A submitted transaction is known
   to the RPC/validator that receives it before it is included, and the
   resulting state is public immediately. Ordering within a ledger is
   validator-determined, so the window between "I see the action" and "I
   respond" is at least one ledger, and responses can be submitted
   pre-signed so they are ready at the boundary.

Two consequences follow. First, the practical question is not "can I see
it?" — for a counterparty, the answer is usually yes by construction.
Second, **deadline granularity is one ledger timestamp** (roughly 5
seconds), and all transactions in a ledger share it. Two competing calls
can therefore both be legal in the same ledger, with the outcome decided
purely by intra-ledger order. That is the race surface.

## 2. Correction: a dispute does not freeze tranche claims

`docs/dispute-scope.md` says the freeze blocks "every other state-changing
call" because each one matches a required status and `Disputed` is never
among them. That is true for every *status-gated* call, but not for
`claim_advance_1/2` or `reclaim_advance_1/2`. Their internals
(`claim_tranche` / `reclaim_tranche`) never check `status`:

```
// claim_tranche
tranche_deadline != 0
!claimed && !expired
now <= deadline
cooperative auth
// reclaim_tranche: same, with now > deadline and buyer auth
```

So while a commitment is `Disputed`:

| Call | Blocked by the freeze? |
| --- | --- |
| `claim_advance_1`, `claim_advance_2` | **No** — still legal within the claim window |
| `reclaim_advance_1`, `reclaim_advance_2` | **No** — still legal after the window lapses |
| `settle`, both `expire_*`, `reclaim_on_nondelivery`, `confirm_delivery`, `fund_remainder`, `fund_fx_shortfall`, `resolve_fx_shortfall`, `cancel`, `reassign_buyer`, `mark_checkpoint`, `release_advance_*`, `ready_for_delivery` | Yes — all require a non-`Disputed` status |

The doc-level correction is applied in
[`dispute-scope.md`](./dispute-scope.md) §2. For this issue, the
consequence is narrow but useful: **"race to claim a tranche before a
dispute freeze lands" is not a viable attack.** A flagger gains no
protection from a claim or a reclaim. Any tranche race in this contract
is about the *deadline* boundary and the default sweep, not about the
dispute.

## 3. The races that do move money

### 3.1 `confirm_delivery` vs. `reclaim_on_nondelivery` — the whole remainder

State: `ReadyForDelivery`, remainder funded (so the contract holds the
cooperative's 650,000 and the buyer has already paid `T = 1,000,000`),
`delivery_deadline` just passed.

| Call | Guards | On success |
| --- | --- | --- |
| `confirm_delivery(quantity, grade)` — operator | `status == ReadyForDelivery`, `remainder_funded`, valid grade index. **No deadline check.** | `Delivered` → `settle` pays the cooperative per `settlement_bps` |
| `reclaim_on_nondelivery` — buyer | status in `Locked`…`ReadyForDelivery`, `now > delivery_deadline` | `Forfeited`, whole balance back to the buyer; cooperative gets +1 strike |

Both are legal in the same ledger. Whichever lands first changes the
status and makes the other fail with `InvalidState`. The value at stake
is the entire remaining escrow — 650,000 in the running example — plus
the cooperative's strike record.

Why this is a *dispute* race and not just a coincidence: `flag_dispute`
is legal from `ReadyForDelivery` even after `delivery_deadline`. A
cooperative that sees (or anticipates) the buyer's reclaim can flag
first, which blocks `reclaim_on_nondelivery` for the full dispute
window. It then has that window to obtain the operator's late
attestation. When the dispute expires, the status is restored to
`ReadyForDelivery` and `confirm_delivery` succeeds. The buyer's only
counter is to re-submit after expiry — but the cooperative can flag
again, and the operator can attest at any point in between.

This is the highest-severity finding in this review: a late, arguably
invalid delivery can be converted into a full settlement by a single
flag plus a cooperating operator, and the buyer has no on-chain way to
make its forfeiture right exclusive.

**Fix direction (product decision, not implemented):** add a
`delivery_deadline` guard to `confirm_delivery` (or give the buyer a
short, exclusive reclaim window after the deadline before late delivery
stops being accepted). That single guard removes the race and most of
issue #118's payoff.

### 3.2 `expire_remainder_window` vs. `reclaim_advance_*` — the lapsed tranche

State: `ReadyForDelivery`, remainder unfunded, `remainder_deadline`
passed, and one advance's claim window has lapsed but the buyer has not
reclaimed it. Number: tranche 2 = 200,000 lapsed, tranche 1 = 150,000
already claimed. The contract holds 200,000.

| Order | Buyer receives | Cooperative receives |
| --- | --- | --- |
| `reclaim_advance_2` first, then `expire_remainder_window` | 200,000 | 150,000 (already claimed); sweep finds 0 |
| `expire_remainder_window` first | 0 | 350,000 (150,000 claimed + 200,000 swept); the later `reclaim_advance_2` fails on an empty balance |

This is worse than a simple "first wins" race: **both transactions can
succeed in the same ledger**, and the split depends on intra-ledger
order. `expire_remainder_window` sweeps the whole token balance and does
**not** require the advance tranches to be resolved the way `settle`
does. The buyer's `reclaim` right is already vested once the window
lapses; the sweep is the only thing that can destroy it, and it destroys
it by moving money the buyer was entitled to.

A cooperative (or any watcher acting for it) can deliberately wait until
the tranche window lapses and then fire the sweep at the moment
`remainder_deadline` passes, capturing both the default penalty *and*
the lapsed tranche. The buyer's defense is to reclaim the instant the
window lapses — which is a submission-timing race, not a right.

**Fix direction:** mirror `settle`'s `TrancheUnresolved` requirement in
`expire_remainder_window` and `expire_fx_shortfall_window`, so a sweep
cannot run while a tranche is unresolved. That makes the destination of
every stroop the result of an explicit `claim`/`reclaim`, which is
exactly the principle `settle`'s doc comment already establishes.

### 3.3 `resolve_fx_shortfall` — first caller fixes the rate

`resolve_fx_shortfall` is permissionless, one-shot (`FxAlreadyResolved`),
and reads whatever fresh Reflector quote is current when it lands. The
converted obligation is

```
fx_adjusted_total = denominated_amount * settlement_bps/10_000 * price / 10^decimals
owed_from_remainder = max(fx_adjusted_total - claimed_by_coop, 0)
shortfall = max(owed_from_remainder - escrowed_balance, 0)
```

A higher `price` (settlement token per unit of the deal's currency) means
a larger shortfall, which the cooperative wants and the buyer does not.
The only decision is *when* the single resolution happens, and both
parties can call it. Whoever lands first at their preferred quote locks
the number for the life of the commitment; the loser cannot re-resolve.

Concrete: escrow 1,000,000, a rate move that implies a 300,000 shortfall
at today's quote and 380,000 at tomorrow's. The cooperative wants to
resolve today (and be paid the larger amount after the buyer's top-up);
the buyer wants to wait (or resolve on a dip). A buyer that sees the
cooperative's resolution pending can front-run it only by submitting its
own resolution in an earlier/equal position — it cannot make the
cooperative's call fail (there is no access control) and it cannot undo
a landed resolution. A `flag_dispute` can block the resolution for one
window, but after expiry the race resumes; the flag buys time, not the
number.

There is no deadline by which resolution must happen, so the party that
prefers delay can simply not call it. The party that prefers a number
calls as soon as its preferred quote appears. This is a game of chicken
decided by oracle movement, not by an on-chain parameter.

**Fix direction:** make the conversion deterministic from a value fixed
earlier (e.g. resolve against the oracle quote at `confirm_delivery`
time, or bound resolution to a short window with one recorded quote), or
give the buyer a cure right that is priced off the same snapshot. Any of
these removes the first-caller advantage.

### 3.4 `flag_dispute` vs. `settle` / `expire_*` / `reclaim_on_nondelivery`

`flag_dispute` is a single-signature, zero-cost call that changes the
status to `Disputed`. Every status-gated call thereafter rejects. So a
party that sees any of the following pending can front-run it with a
flag:

| Pending call | Flagging party who benefits | What the flag buys |
| --- | --- | --- |
| `settle` | buyer disputing the grade | blocks the cooperative's remainder payment for one window; leverage, not a different number |
| `expire_remainder_window` | buyer about to default | postpones its own permanent bar and the sweep, one window at a time |
| `reclaim_on_nondelivery` | cooperative about to forfeit | the window needed to obtain a late `confirm_delivery` (§3.1) |
| `expire_fx_shortfall_window` | buyer refusing the top-up | postpones `Defaulted` and the bar |
| `resolve_fx_shortfall` | either side, depending on the rate | denies the counterparty the chance to lock a quote during the window |
| `cancel` / `reassign_buyer` | either party | delay only; both need all signatures anyway |

The flag cannot be countered except by waiting for `expire_dispute_window`
and resubmitting, and a new flag can land immediately after expiry. The
one-block race is real; the strategy it enables is the subject of
[`dispute-stalling.md`](./dispute-stalling.md).

## 4. Races that do not exist (and why that matters)

| Candidate race | Why it is not exploitable |
| --- | --- |
| `claim_advance_*` vs. `reclaim_advance_*` at the window boundary | The guards are strict complements (`now <= deadline` vs. `now > deadline`), so exactly one is legal at any timestamp. A claim submitted just before the boundary that lands just after fails — the cooperative's own timing risk, not a counterparty attack. No third party gains. |
| Racing `settle` | Permissionless and deterministic on recorded state; the caller is not paid and no argument is accepted. Reordering changes nothing. |
| Racing `release_advance_1/2` | Starts a clock, moves no funds; no outcome depends on who calls. |
| Racing `cancel` / `reassign_buyer` | Both require every affected party's signature in the same invocation; there is no unilateral action to race. |
| A third party backrunning a permissionless call for profit | Every transfer names `buyer` or `cooperative`; there is no caller reward, no slippage, and no parameter to manipulate. A searcher can only choose *when* a fixed party gets paid. |
| Racing `resolve_dispute` vs. `expire_dispute_window` | Mutually exclusive on `status`; the loser fails with `InvalidState`, and the restored status is identical either way (pinned by `dispute_resolve_and_expire_race_leaves_consistent_state_regardless_of_order`). No economic difference. |

The last two rows are the reassuring half of the finding: this contract
has no generalized extractable value. Its races are **specific,
counterparty-only, and traceable to two missing guards** — the delivery
deadline on `confirm_delivery`, and the tranche-resolution requirement on
the sweeps — plus the one-shot oracle read.

## 5. Severity and recommended order of work

| Finding | Value at stake | Who can exploit | Severity |
| --- | --- | --- | --- |
| Late `confirm_delivery` beats forfeiture (§3.1) | Entire remaining escrow (650,000 in the example) | Cooperative, with operator attestation | **High** |
| Sweep eats a lapsed tranche (§3.2) | One advance (15–20% of `T`) | Cooperative / any watcher | **Medium-high** |
| First-caller FX resolution (§3.3) | The FX shortfall delta | Either economic party | **Medium** |
| Flag blocks a status-gated call (§3.4) | One window of delay; enables §3.1 | Any of the three parties, repeatable | **Medium** |
| Claim/reclaim boundary miss (§4) | One advance, but self-inflicted | Nobody — no counterparty gain | Low |
| Third-party extraction | None possible | — | None |

Suggested sequencing if any of this becomes code work: add the
`delivery_deadline` guard first (it is a one-line guard with a large
value at stake and no product ambiguity), then the tranche-resolution
requirement on both sweeps (it extends a principle `settle` already
implemented), then decide whether one-shot oracle resolution is the
product behavior the pilot wants. The flag bond and re-flag cap are
product decisions with a wider blast radius; they belong with the
stalling analysis in [`dispute-stalling.md`](./dispute-stalling.md) §4.

This issue asked for a write-up, not a code change, so none of these
fixes are implemented here. The races are documented with the exact
guards that create them so the eventual fix is a reviewable delta rather
than a rediscovery.

Where this revises [`threat-model.md`](./threat-model.md): verdict 14 gains
the `claim`/`reclaim` exception (§2), verdict 16 loses its "bounded" claim
(§3.4), verdict 18 becomes "partially defended" because one-shot resolution
is still a first-caller race (§3.3), and the out-of-scope "front-running /
MEV around permissionless calls" row becomes in-scope for the two
order-dependent transfers in §3.1 and §3.2. Each change carries the reason
in the threat model's own entry, per its maintenance section.
