# Dispute stalling — does a losing party gain by flagging a spurious dispute?

Issue #118 asked whether a party losing on the merits of a deal has a real
incentive to call `flag_dispute` just to buy time, and whether the
three-party resolution requirement is sufficient deterrence. Short
answer: **the three-party requirement is not deterrence at all — it is
the mechanism that makes the stall work — and the stall is both cheap
and repeatable.**

Ground truth is `contracts/escrow/src/lib.rs`
(`flag_dispute` / `resolve_dispute` / `expire_dispute_window`) and
`api/src/reputation.ts` in the sibling `HarvestLock` repo. Companion docs:
[`dispute-scope.md`](./dispute-scope.md) (what a dispute is and is not),
[`dispute-frontrunning.md`](./dispute-frontrunning.md) (which races the
flag participates in), and
[`griefing-and-inaction.md`](./griefing-and-inaction.md) (inaction that
does not even need a flag).

## TL;DR

- `flag_dispute` is **free**: no bond, no deposit, no fee, no reputation
  event, no per-commitment limit. The only cost is one transaction.
- The freeze is bounded by `remainder_window_secs` (1 hour to 30 days
  through the API; any positive value at the contract), and
  `expire_dispute_window` is permissionless — **but nothing stops the
  same party from re-flagging immediately after expiry**. Each cycle
  costs one transaction and buys another full window. The freeze is
  therefore bounded per flag and unbounded in aggregate.
- **Unanimous resolution makes the flagger's own refusal decisive.**
  `resolve_dispute` requires all three parties including the flagger, so
  the flagger can always force the victim to wait out the window. The
  "three-party requirement" raises the cost of *ending* a dispute, not
  the cost of *starting* one. It is an anti-unilateral-resume rule, not
  a deterrent.
- The freeze is **not total**: `claim_advance_1/2` and
  `reclaim_advance_1/2` never check `status`, so they still work while
  `Disputed`. Every *status-gated* transition is blocked; the two
  deadline-gated tranche actions are not. See §2.
- A flag **does not pause any deadline**. The defaulter cannot use it to
  cure a payment that is already late (`fund_remainder` and
  `fund_fx_shortfall` still reject after their deadlines). What it buys
  is delay, optionality, and — in one case — a genuine outcome flip.
- The strongest concrete incentive is **the cooperative avoiding
  `Forfeited`**: after `delivery_deadline`, `confirm_delivery` has no
  deadline guard, so a flag can block the buyer's reclaim just long
  enough for the operator to attest late. Value at stake: the entire
  remaining escrow (§3.1).
- The strongest buyer incentive is **delaying its own permanent bar**:
  while `Disputed`, the status is not `Defaulted`, so the off-chain
  consequence never fires. Re-flagging every window postpones the bar
  indefinitely (§3.2).
- The warehouse operator can flag, has no funds at stake, and has no
  reputation consequence. It is the only party that can freeze a deal
  forever at zero cost to itself (§3.4).

## 1. The mechanics, with cost and benefit separated

| Property | Value | Consequence |
| --- | --- | --- |
| Who can flag | any one of buyer, cooperative, operator (own auth) | one signature, no co-signer |
| Cost to flag | one transaction fee; no bond, no slash, no standing event | ~free relative to any real deal |
| Reachable from | `Locked` … `Delivered` | includes the two moments that matter: just before a default sweep, just before forfeiture |
| Effect | status → `Disputed`; every status-gated call rejects | blocks settlement, both expiry sweeps, forfeiture, cancel, reassign, delivery confirmation, FX resolution/funding |
| Not affected | `claim_advance_*`, `reclaim_advance_*`, all deadlines | tranche money can still move; the clock never stops |
| Ends by | unanimous `resolve_dispute` (all three, **including the flagger**) or permissionless `expire_dispute_window` after the window | the flagger can always force the timeout path |
| Window length | `remainder_window_secs` | 1 hour to 30 days via the API; unbounded in the contract |
| Re-flag after expiry | allowed immediately — status is restored, and `flag_dispute` has no cooldown or counter | indefinite serial stalling |
| Off-chain consequence | **none** — `api/src/reputation.ts` only records `Defaulted` and `Forfeited` | flagging is invisible to reputation |

The decisive asymmetry is in the "ends by" row. `resolve_dispute` needs
the flagger's signature, so the flagger controls whether the fast path
is available. `expire_dispute_window` exists precisely so the other two
parties are not permanently hostage — but it only converts an indefinite
freeze into a series of fixed-length freezes. Nothing in the contract
charges the flagger for the delay.

## 2. The freeze is leaky, and that cuts both ways

Every status-gated function is blocked because each matches an explicit
required status and `Disputed` is never one of them. But
`claim_tranche` and `reclaim_tranche` (the internals behind
`claim_advance_1/2` and `reclaim_advance_1/2`) do **not** check `status`
at all. Their only guards are:

```
tranche_deadline != 0        // opened
!claimed && !expired         // one-shot
now <= deadline (claim) / now > deadline (reclaim)
```

So during a dispute:

- the cooperative can still claim an open tranche;
- the buyer can still reclaim a lapsed one;
- a dispute cannot protect a tranche from either side, and
- a dispute cannot prevent the tranche's clock from being acted on.

This matters for the stalling question because it removes two motives
that would otherwise seem obvious ("freeze so my tranche can't be
reclaimed" / "freeze so their claim can't land"). Neither works. What a
dispute actually freezes is the set of calls that change `status` or move
the *undisbursed* balance. For a full list, see
[`dispute-scope.md`](./dispute-scope.md) §2; the correction that
claim/reclaim are not blocked was added in this review.

## 3. Who actually gains, with concrete scenarios

Numbers follow [`terminal-outcomes.md`](./terminal-outcomes.md):
`T = 1,000,000`, advances 15% + 20% (`D = 350,000`), remainder
`R = 650,000`, remainder/dispute window 7 days.

### 3.1 The cooperative: flip `Forfeited` into `Settled` after the deadline

This is the real money case. The cooperative has delivered late (or not
at all), the buyer is about to call `reclaim_on_nondelivery`, and the
contract is in `ReadyForDelivery` with the remainder funded. The
cooperative's choice set:

| Action | Outcome | Cooperative receives |
| --- | --- | --- |
| Do nothing | buyer calls `reclaim_on_nondelivery` → `Forfeited` | 350,000 already claimed; +1 strike |
| `flag_dispute` before the buyer's call lands | buyer's reclaim rejects (`InvalidState`); cooperative now has one full window | still 350,000 for now, no strike yet |
| Flag, then have the operator call `confirm_delivery` after expiry | status restored to `ReadyForDelivery`, operator attests late (no deadline guard), `settle` runs | 1,000,000 total, no strike |

The flag buys the only thing that was missing: time for a late
attestation. `confirm_delivery` checks `status == ReadyForDelivery`,
`remainder_funded`, and the grade index — **not** `delivery_deadline`.
`reclaim_on_nondelivery` checks that the deadline has passed. So after
the deadline both calls are legal, and whichever lands first decides
whether the remaining 650,000 goes to the cooperative or back to the
buyer. A 7-day (or 30-day) freeze is more than enough to organize the
operator's signature.

The three-party resolution requirement does not deter this. The
cooperative is the flagger and will simply refuse to sign
`resolve_dispute`; the buyer can only wait out the permissionless expiry,
then try again — and the cooperative can flag again if it still needs
time. The buyer's recourse is a race, not a right.

Caveat on severity: this requires the operator to cooperate with a late
attestation. Nothing in the contract punishes the operator for attesting
late, attesting a low grade, or never attesting at all — see §3.4 and
[`griefing-and-inaction.md`](./griefing-and-inaction.md). So the
cooperative's plan needs an accomplice, but the accomplice has no
downside.

### 3.2 The buyer: postpone the permanent bar, and block settlement

The buyer's on-chain position after missing the remainder deadline is
already lost: `expire_remainder_window` sweeps the balance to the
cooperative and `fund_remainder` rejects any payment after the deadline,
so no amount of stalling cures the default. But the reputation
consequence is triggered by observing a **transition into `Defaulted`**.
A commitment sitting in `Disputed` is not `Defaulted`, and
`api/src/reputation.ts` records nothing for it.

So the buyer's rational play, once it knows it cannot pay, is:

1. call `flag_dispute` from `ReadyForDelivery` before anyone calls
   `expire_remainder_window`;
2. wait out the 7-day dispute window (the cooperative cannot force
   `resolve_dispute` without the buyer's signature);
3. when the cooperative calls `expire_dispute_window`, submit a fresh
   flag in the same ledger window, before the cooperative can land
   `expire_remainder_window`.

Each cycle costs the buyer one transaction and one week of the
cooperative's capital being stuck in the contract. Step 3 is a race, not
a guaranteed veto — the cooperative can try to land `expire` and
the sweep back to back, and the buyer can lose a cycle. But every cycle
the buyer wins postpones the permanent bar, and there is no contractual
limit on how many times it can try. The cooperative still gets the money
eventually — but its funds are illiquid the whole time. This is the
cleanest demonstration that the expiry hatch bounds a single freeze but
not the strategy.

The buyer has a second, smaller motive: **blocking `settle`** after a
delivery it considers over-graded. `settle` is the only way the
cooperative receives the remainder; a buyer holding the goods can freeze
it to create negotiation leverage even though the on-chain
`settlement_bps` is fixed. The buyer's own refund is frozen too, so this
is a credibility play, not free money. It becomes rational when the
disputed amount exceeds the time value of the buyer's refund.

A third motive that *does not* work: blocking `resolve_fx_shortfall` to
choose the rate. The freeze does block the one-shot resolution, but
after expiry either party can call it, so the flag only denies the
counterparty the chance to lock a rate during the window — it does not
let the flagger pick the number. That race is covered in
[`dispute-frontrunning.md`](./dispute-frontrunning.md) §4.

### 3.3 The buyer: delay a forfeiture? No.

The mirror of §3.1 does not exist for the buyer. After
`delivery_deadline`, the buyer wants `Forfeited`; flagging would block
its own `reclaim_on_nondelivery` and hand the cooperative exactly the
time it needs to confirm delivery late. Flagging in that position is
self-harm. The buyer's useful stall window is *before* its own default
(§3.2), not after the cooperative's.

### 3.4 The operator: freeze anything, forever, for free

The operator is a named party, can call `flag_dispute`, has no token at
stake in any outcome, and has no reputation event type in
`api/src/db/reputation.ts`. It can therefore:

- block `settle` indefinitely by re-flagging every window, denying the
  cooperative its remainder and the buyer its refund;
- block `reclaim_on_nondelivery`, denying the buyer its refund;
- block `cancel`, denying both parties the clean unwind;
- do all of it without ever touching a token or generating a record.

There is no on-chain cost and no off-chain consequence. The only check
is that the operator cannot *take* anything — every path still pays a
named party. This is a pure liveness attack, but it is a complete one.

## 4. Is the three-party requirement sufficient deterrence?

No, and the reason is structural rather than a matter of degree:

1. **It is a consent rule for ending a freeze, not a price for starting
   one.** The flagger is one of the three consents, so the requirement
   *guarantees* the flagger a veto over the fast path. That is the
   opposite of deterrence.
2. **The escape hatch is not a penalty.** `expire_dispute_window` is
   permissionless and restores the pre-dispute status exactly. It costs
   the flagger nothing; it costs the victim a window. The victim cannot
   recover the delay or charge for it.
3. **Re-flagging is unconstrained.** No counter, cooldown, bond, or
   escalating window exists. The strategy is "pay gas, repeat".
4. **The reputation layer cannot see it.** Only `Defaulted` and
   `Forfeited` produce standing events, so a party that weaponizes
   disputes but never defaults is invisible. If anything, the mechanism
   rewards it: stalling can *prevent* the very status that would be
   recorded against the other side.
5. **The flagger's own exposure is asymmetric by role.** A cooperative
   that stalls to avoid a forfeiture protects its 3-strike record; a
   buyer that stalls to postpone a default protects a first-offense
   permanent bar. The party with more to lose from a status has more to
   gain from the freeze.

What would change the incentive (product decisions, not implemented
here, listed so the gap is explicit rather than implied):

- a **bond** paid by the flagger and forfeited to the counterparties if
  the dispute lapses through `expire_dispute_window` rather than
  unanimous resolution;
- a **per-commitment flag cap** or escalating dispute window, so
  re-flagging becomes monotonically more expensive;
- **shifting every other deadline by the dispute duration** (the known
  limitation in `flag_dispute`'s doc comment), which removes the "freeze
  to win a race at the deadline" value rather than only bounding it;
- recording `flag_dispute` / `expire_dispute_window` in the reputation
  layer, so a repeat staller accumulates a visible history even without
  a terminal status;
- a **deadline guard on `confirm_delivery`**, which removes §3.1's
  outcome flip entirely (the late-delivery race in
  [`dispute-frontrunning.md`](./dispute-frontrunning.md) §2).

None of these are required to close this issue — the issue asked for the
analysis, and the analysis is that the current mechanism is a cheap,
repeatable, unpunished delay for the party who benefits most from
delay.

This revises verdict 16 in [`threat-model.md`](./threat-model.md), which
rated the same attack "accepted risk, bounded": the timeout bounds a single
freeze, but nothing in the contract bounds the number of freezes, and the
late-delivery case in §3.1 means a pause can change an outcome rather than
only delay one.

## 5. What a stall cannot do

To keep the finding in proportion:

- It cannot move funds to the flagger. Every payout still goes to the
  buyer or cooperative named in the commitment.
- It cannot cure a missed payment deadline. `fund_remainder` and
  `fund_fx_shortfall` still compare against the original deadline, which
  does not pause.
- It cannot extend a claim window, so it cannot create a new claim
  right.
- It cannot stop `claim_advance_*` / `reclaim_advance_*` (§2), so it is
  not a way to protect a tranche.
- It cannot prevent the eventual `Defaulted`/`Forfeited` if the other
  side keeps calling the expiry path; it can only delay it and, in the
  forfeiture case, create the window for a late confirmation.

The finding is about **time, optionality, and reputation-record timing**,
not about direct theft. In a deal where the counterparty is
cash-constrained at harvest, time is exactly the asset worth attacking.
