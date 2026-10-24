# Terminal outcomes — exactly what each party walks away with, on-chain and off-chain

Issue #119 asked for one reference table: for `Settled`, `Defaulted`,
`Forfeited`, and `Cancelled`, what do the buyer, the cooperative, and the
warehouse operator actually end up with, cross-referenced against the
off-chain reputation consequence that layers on top. This doc is that
table, plus worked scenarios, because "the cooperative gets the balance"
is not a useful sentence to a party deciding whether to sign.

Ground truth is `contracts/escrow/src/lib.rs` (the guards and payout math)
and the sibling `HarvestLock` repo's `api/src/reputation.ts` /
`api/src/db/reputation.ts` (fetched from `main` on 17 Oct 2026; that repo
is not vendored here). Where the two disagree, the contract decides who
gets paid and the API decides what is recorded about it — they cannot
disagree about an amount, only about whether anyone remembers it.

Companion docs from the same review pass:
[`griefing-and-inaction.md`](./griefing-and-inaction.md) (what each party
can do by doing nothing), [`dispute-stalling.md`](./dispute-stalling.md)
(whether a spurious flag pays), and
[`dispute-frontrunning.md`](./dispute-frontrunning.md) (which pending
calls are worth racing). The risk-register companion is
[`threat-model.md`](./threat-model.md); §7 below is the source for its
reputation-consequence entries, and it is where the two docs disagree about
whether an outcome can be changed by pausing (it cannot, with the
late-delivery exception in §5.4).

## TL;DR

- **The warehouse operator never receives a token from the contract in any
  terminal status, and never receives an off-chain reputation record
  either.** Its economics are entirely off-chain (its own fee from the
  buyer or cooperative) and are not modeled here.
- **`Settled` is the only status where the cooperative can receive more
  than it already claimed**, and the only one where the buyer can get a
  refund larger than an unclaimed deposit.
- **`Defaulted` always sweeps 100% of the contract balance to the
  cooperative**, including any advance tranche whose claim window has
  lapsed but which the buyer has not yet reclaimed. That is the buyer's
  most valuable right to lose, and it is lost by inaction, not by a
  judgment of fault.
- **`Forfeited` always sweeps 100% of the contract balance back to the
  buyer**, and never claws back an advance the cooperative already
  claimed. The cooperative's forfeiture cost is therefore capped at
  "unpaid remainder plus the strike", not "everything".
- **`Cancelled` and `Settled` carry no reputation consequence at all.**
  Only a fresh transition into `Defaulted` (buyer) or `Forfeited`
  (cooperative) is recorded. A party that stalls forever in a
  non-terminal status has no reputation record, because there is no
  transition to observe.
- **The off-chain consequences are wildly asymmetric**: first buyer
  default is an immediate permanent bar; a cooperative forfeiture is one
  of three strikes before a bar. Neither is enforced against the
  warehouse operator.

## 1. Notation and the baseline deal

Every scenario below uses the same commitment, so the numbers are
comparable across statuses:

| Term | Value | Notes |
| --- | --- | --- |
| `total_amount` (`T`) | 1,000,000 | 7-decimal settlement token, e.g. 0.1 USDC at 7 dp |
| `advance1_bps` | 1,500 | 15% released at lock-in |
| `advance2_bps` | 2,000 | 20% released at the checkpoint |
| Deposit `D` | 350,000 | what `lock` escrows: `T * (a1 + a2) / 10_000` |
| Remainder `R` | 650,000 | what `fund_remainder` escrows: `T - D` |
| `grade_price_bps` | `[10_000, 9_000, 7_500]` | grade A / B / C multipliers |
| `delivery_window_secs` | 120 days | the app-level default |
| `claim_window_secs` | 7 days | |
| `remainder_window_secs` | 7 days | also the dispute and FX-shortfall window |
| `oracle_config` | none | unless the FX scenario says otherwise |

Two conventions matter when reading the tables:

- **Cash vs. value.** The contract only moves tokens. In a `Settled`
  scenario the buyer also holds physical goods; in a `Forfeited` or
  `Defaulted` scenario it usually does not (or holds goods it has no
  on-chain title to). The tables show cash; the prose names the goods.
- **"Already claimed" is not the same as "earned".** Once
  `claim_advance_*` pays the cooperative, no terminal status claws it
  back — `settle` explicitly floors `owed_from_remainder` at zero rather
  than recovering an over-advance, and `cancel`/`Forfeited` never touch
  paid advances. The contract's downside protection for the buyer is the
  *unpaid* remainder, never the paid advances.

## 2. The four terminal statuses at a glance

| Status | Reached by | Who can trigger it | Contract balance goes to | Already-claimed advances | Off-chain record |
| --- | --- | --- | --- | --- | --- |
| `Settled` | `Delivered` + both tranches resolved (+ FX resolved and funded if configured) | **Anyone** (`settle` is permissionless) | Split: cooperative gets `min(adjusted_total − claimed, balance)`, buyer gets the rest as a refund | Never clawed back | None |
| `Defaulted` | `ReadyForDelivery` past `remainder_deadline` unfunded, **or** `Delivered` with an unfunded FX shortfall past `fx_shortfall_deadline` | **Anyone** (both `expire_*` are permissionless) | **100% to the cooperative** | Kept | Buyer: `buyer_default` → **immediate, permanent bar** |
| `Forfeited` | Any state from `Locked` through `ReadyForDelivery`, past `delivery_deadline`, without `confirm_delivery` | **Buyer only** (`reclaim_on_nondelivery`) | **100% to the buyer** | Kept | Cooperative: `cooperative_forfeiture` → strike; **bar on the 3rd** |
| `Cancelled` | Any state from `Draft` through `ReadyForDelivery` | Buyer **and** cooperative together | **100% to the buyer** | Kept | None |

`Settled`, `Defaulted`, `Forfeited`, and `Cancelled` are absorbing: no
function accepts them as an input status, so nothing moves after one
lands. `Disputed` is not terminal — it is a bounded pause, covered in
[`dispute-scope.md`](./dispute-scope.md).

## 3. `Settled`

Preconditions: `confirm_delivery` ran (so `settlement_bps` is fixed),
both tranches are resolved, and, for an oracle-configured deal,
`resolve_fx_shortfall` ran and any shortfall was funded.

Payout formula, verbatim from `settle`:

```
adjusted_total    = fx_adjusted_total            if oracle-configured
                  = T * settlement_bps / 10_000  otherwise
claimed_by_coop   = (a1 share if advance1_claimed) + (a2 share if advance2_claimed)
owed_from_remainder = max(adjusted_total - claimed_by_coop, 0)
coop_payment      = min(owed_from_remainder, contract token balance)
buyer_refund      = contract token balance - coop_payment
```

### 3.1 Full delivery, full grade

Both advances claimed (350,000 total), remainder funded (650,000),
`settlement_bps = 10_000`:

| Party | Receives | Net cash position |
| --- | --- | --- |
| Buyer | 0 refund | paid 1,000,000; holds the full crop |
| Cooperative | +650,000 now; 350,000 already claimed | **1,000,000 total** |
| Operator | 0 | unchanged |

### 3.2 Half delivery at grade B

`delivered_quantity = 500 / 1,000`, `grade_index = 1` (9,000 bps):
`settlement_bps = 5_000 * 9_000 / 10_000 = 4_500`, so
`adjusted_total = 450,000`. Both advances claimed (350,000):

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 550,000 refund | paid 450,000 for half the crop at grade B |
| Cooperative | 100,000 now + 350,000 claimed | **450,000 total** |
| Operator | 0 | unchanged |

The 550,000 refund is not "the buyer's deposit back" — it is the entire
unowed remainder, and it can only be that large because the buyer funded
the remainder first. An unfunded remainder has no `settle` path at all;
it is a `Defaulted` path.

### 3.3 Severe shortfall: the advance is not clawed back

`settlement_bps = 2_000` (a near-total write-off), both advances already
claimed (350,000), remainder funded:

`adjusted_total = 200,000`, `claimed_by_coop = 350,000`,
`owed_from_remainder = 0`. The cooperative keeps the 350,000 it already
claimed and receives nothing more; the buyer gets the whole 650,000
remainder back.

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 650,000 refund | paid 350,000 for goods the contract values at 200,000 |
| Cooperative | 0 now + 350,000 claimed | **350,000 total**, i.e. 150,000 more than the delivery earned |
| Operator | 0 | unchanged |

This is the documented PRD §7 "advance not clawed back" rule, and it is
the cooperative's single strongest protection: the worst a disputed grade
can do to money it has already received is nothing.

### 3.4 A lapsed, unreclaimed tranche makes the cooperative absorb the loss

The subtle case. Tranche 1's window lapses; the cooperative never claims
it and the buyer does not immediately reclaim. The buyer reclaims
150,000 (`reclaim_advance_1`), the cooperative claims tranche 2
(200,000), the buyer funds the remainder (650,000), and delivery is full
at full grade:

| Step | Contract balance | Cooperative | Buyer |
| --- | --- | --- | --- |
| after `lock` | 350,000 | 0 | −350,000 |
| `reclaim_advance_1` | 200,000 | 0 | −200,000 |
| `claim_advance_2` | 0 | 200,000 | −200,000 |
| `fund_remainder` | 650,000 | 200,000 | −850,000 |
| `settle`: adjusted 1,000,000 − claimed 200,000 = 800,000 owed, only 650,000 present | 0 | **850,000** | −850,000 net |

The buyer paid 850,000 for a crop the contract valued at 1,000,000, and
the cooperative received 850,000 instead of 1,000,000. The missing
150,000 is tranche 1, which the buyer reclaimed and the cooperative
forfeited by missing its own claim window. There is no clawback and no
recovery: `settle` cannot collect money that has already left the
contract. **A cooperative that misses a claim window permanently donates
that tranche to the buyer, even after perfect delivery.** This is the
mirror image of §3.3 and the reason both tranches must be resolved before
`settle`: the contract refuses to guess which side a lapsed tranche
belongs to, and the answer depends on whether a `reclaim` already
happened.

### 3.5 Oracle-configured settlement

With an oracle, `adjusted_total = fx_adjusted_total`, computed once by
`resolve_fx_shortfall` from a fresh Reflector quote. Example from the
contract's own doc comment: a 1,000,000 NGN obligation reprices to
667 USDC at ₦1,500/USD; if only 650 USDC was escrowed, the buyer must
fund a 17 USDC shortfall before `settle` runs. If the buyer funds it, the
cooperative receives the full fresh-rate value and the buyer's FX risk is
realized as a cash cost. If the buyer does not, see §4.2.

## 4. `Defaulted`

`Defaulted` has two triggers, both deadline-based and both permissionless
to fire. They have the same balance destination and the same reputation
consequence, but very different economics because of *when* they happen.

### 4.1 Remainder default — `expire_remainder_window`

State: `ReadyForDelivery`, `remainder_deadline` passed, remainder never
funded. `reclaim_on_nondelivery` is *not* the trigger here; it needs the
later `delivery_deadline`.

Scenario: tranche 1 claimed (150,000), tranche 2's window lapsed and the
buyer never reclaimed (200,000), remainder never funded.

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 0 | paid 350,000, received no goods — **including the 200,000 it had a vested right to reclaim** |
| Cooperative | 200,000 swept + 150,000 already claimed | **350,000 total** |
| Operator | 0 | unchanged |

The 200,000 sweep is the sharp edge. `expire_remainder_window` transfers
the *whole contract balance*, not "the deposit minus what the cooperative
is owed", and it does not require the tranches to be resolved the way
`settle` does. If tranche 2's window had lapsed, the buyer already held
the right to `reclaim_advance_2` for 200,000. The sweep converts that
vested right into the cooperative's money. A buyer who reclaims before
the sweep keeps it; one who waits loses it. The race is documented in
[`dispute-frontrunning.md`](./dispute-frontrunning.md).

If the buyer had reclaimed the lapsed tranche first, the contract
balance at default would be 0 and the cooperative would keep only the
150,000 it had claimed. **The default penalty is therefore not a fixed
amount — it is whatever happens to be sitting in the contract when
someone calls `expire_remainder_window`.**

### 4.2 FX-shortfall default — `expire_fx_shortfall_window`

State: `Delivered`, goods already attested and in the buyer's hands,
`fx_shortfall_amount > 0`, `fx_shortfall_deadline` passed, top-up never
funded. Full funding at the old rate: cooperative claimed 350,000,
remainder 650,000 escrowed. Fresh rate makes the deal worth 1,350,000,
so the recorded shortfall is 350,000.

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | keeps the 350,000 it refused to pay; holds goods the contract now values at 1,350,000 | paid 1,000,000 for 1,350,000 of value — **350,000 better off than performing** |
| Cooperative | 650,000 swept + 350,000 claimed | **1,000,000 total**, i.e. the stale-rate value, not the current-rate value |
| Operator | 0 | unchanged |

This is the one terminal outcome where **defaulting is profitable on the
cash line**. The buyer's only additional loss is the permanent bar. The
cooperative's remedy is complete in the sense that it gets everything
still escrowed, but incomplete in the sense that the shortfall it was
owed was never escrowed — it is a post-delivery obligation with no
collateral behind it. With a large enough FX move, a buyer who does not
expect to need another commitment on the same address is rationally
better off refusing the top-up. The product decision to reuse
`Defaulted` (and therefore the permanent bar) is the whole deterrent;
there is no on-chain penalty beyond the escrow the buyer already paid.

### 4.3 What the warehouse operator gets

Nothing, in both triggers. No token transfer ever names the operator in
`sweep` paths, and `api/src/db/reputation.ts` has no operator event type.
An operator that attested a grade the day before an FX default is
untouched by it.

## 5. `Forfeited`

Trigger: `reclaim_on_nondelivery`, called by the buyer, after
`delivery_deadline`, from any of `Locked`, `Advance1Released`,
`CheckpointPassed`, `Advance2Released`, or `ReadyForDelivery`. The whole
contract balance goes to the buyer; claimed advances stay with the
cooperative.

### 5.1 Full funding, then no delivery

Both advances claimed (350,000), remainder funded (650,000), delivery
deadline passes with no `confirm_delivery`:

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 650,000 | paid 1,000,000, received no goods, **lost 350,000** |
| Cooperative | 0 now + 350,000 claimed | **350,000** for a crop it never delivered, plus one strike |
| Operator | 0 | unchanged |

The cooperative's downside is bounded at the unpaid remainder plus the
strike. It keeps the advances because they were paid for early-season
work that may genuinely have happened; the contract has no way to
distinguish "grew the crop and sold it elsewhere" from "never planted",
and deliberately does not try.

### 5.2 Deposit only, one advance claimed

`lock` funded 350,000; tranche 1 claimed (150,000); remainder never
funded; delivery deadline passes:

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 200,000 | lost 150,000, no goods |
| Cooperative | 0 now + 150,000 claimed | **150,000**, one strike |
| Operator | 0 | unchanged |

### 5.3 Deposit only, nothing claimed

Contract balance 350,000, nothing claimed, deadline passes: the buyer
recovers the full 350,000, the cooperative receives 0 — and still records
a forfeiture strike when the buyer calls the function. **The status is a
fault record, not a damages award.**

### 5.4 The cooperative can still avoid this status after the deadline

`confirm_delivery` has **no `delivery_deadline` guard**. It only checks
`status == ReadyForDelivery`, `remainder_funded`, and a valid
`grade_index`. `reclaim_on_nondelivery` checks only that the deadline has
passed and the status is pre-delivery. After `delivery_deadline`, both
calls are legal, and whichever lands first decides the outcome:
`Forfeited` (cooperative keeps only claimed advances, strike) or
`Delivered` → `Settled` (cooperative gets the remainder per
`settlement_bps`). This is not a theoretical edge: it is a one-block
race with the entire remaining escrow as the prize, and
[`dispute-frontrunning.md`](./dispute-frontrunning.md) treats it as the
headline finding of issue #117. `flag_dispute` can be used by the
cooperative to block the buyer's `reclaim_on_nondelivery` for a full
window while the operator's late attestation is arranged.

## 6. `Cancelled`

Trigger: `cancel`, requiring both the buyer and the cooperative in the
same call, from `Draft` through `ReadyForDelivery`. The entire balance
goes back to the buyer; claimed advances stay with the cooperative; no
reputation event exists for this status.

### 6.1 At `ReadyForDelivery`, both advances claimed, remainder funded

| Party | Receives | Net |
| --- | --- | --- |
| Buyer | 650,000 refund | paid 1,000,000, got 650,000 back, no goods — **lost 350,000** |
| Cooperative | 0 now + 350,000 claimed | **350,000**, no strike |
| Operator | 0 | unchanged |

Compare §5.1: the buyer's cash outcome is identical, but the buyer gets
it earlier (no need to wait for `delivery_deadline`), and the
cooperative avoids a forfeiture strike. **That is the deal `cancel`
offers: the buyer trades patience for speed, the cooperative trades a
forfeiture record for the same money.** It is also why a cooperative with
a forfeiture already pending has every reason to prefer `cancel` and a
buyer with leverage has every reason to refuse it and wait.

### 6.2 At `Locked`, nothing claimed

The buyer gets all 350,000 back and the cooperative gets nothing. No
strike. This is the cleanest unwind the contract offers, and it is
available only while both parties still agree.

### 6.3 At `Draft`

Nothing was ever escrowed. The status changes, no tokens move, no
reputation record. The only cost is gas.

## 7. Cross-reference: on-chain outcome vs. off-chain reputation

`api/src/reputation.ts` applies consequences from exactly two statuses:

```ts
const TERMINAL_STATUSES_WITH_CONSEQUENCES = new Set(["Defaulted", "Forfeited"]);
// ...
if (previousStatus === commitment.status) return;      // once per transition
if (commitment.status === "Defaulted")  await recordBuyerDefault(commitment.buyer, contractId);
if (commitment.status === "Forfeited") await recordCooperativeForfeiture(commitment.cooperative, contractId);
```

`api/src/db/reputation.ts` turns those into standing:

| On-chain status | Recorded event | Effect | Bar threshold |
| --- | --- | --- | --- |
| `Defaulted` | `buyer_default` | `barred = TRUE`, `barred_reason = 'buyer_default'` | **First occurrence** |
| `Forfeited` | `cooperative_forfeiture` | `strike_count += 1` | **3rd occurrence** (`cooperative_forfeiture_3x`) |
| `Settled` | none | — | — |
| `Cancelled` | none | — | — |
| `Disputed` | none | — | — |

The resulting matrix, per party:

| Status | Buyer | Cooperative | Warehouse operator |
| --- | --- | --- | --- |
| `Settled` | Refund of unowed escrow; holds the crop; no standing change | Balance of the earned amount + kept advances; no standing change | Nothing on-chain, nothing off-chain |
| `Defaulted` | Loses all escrow still in the contract; **permanently barred on first observation** | Receives all escrow still in the contract; no standing change | Nothing; not even an event type exists |
| `Forfeited` | Recovers all escrow still in the contract; no standing change | Keeps claimed advances; **+1 strike, barred at 3** | Nothing; not even an event type exists |
| `Cancelled` | Recovers all escrow still in the contract; no standing change | Keeps claimed advances; no standing change | Nothing |

Four properties of the off-chain layer are worth stating explicitly,
because they change how the on-chain table should be read:

1. **Observation-triggered, not event-driven.** Consequences are applied
   when the API reads the commitment (`GET /commitments/:id`) or after a
   submitted transaction asks for a refresh. A `Defaulted` that nobody
   reads is not recorded until someone does. The API's own comment calls
   this out; it is a best-effort cache side effect, wrapped in
   `.catch(...)`, so a Postgres failure silently drops the consequence.
2. **Enforced only at creation, only for two roles.** `requireNotBarred`
   is called for `buyer` and `cooperative` in
   `POST /commitments/:contractId/tx/initialize`, and nowhere else. A
   barred buyer can still be **reassigned into** a live commitment
   (`reassign_buyer` has no standing check in the API), and any party can
   bypass the API and sign directly against the contract. The bar is a
   product-level gate on the hosted flow, not an on-chain permission.
   The warehouse operator is deliberately never checked.
3. **There is no appeal mechanism in code.** `standing_events` exists as
   the audit trail a human reviewing an appeal would use (per
   `002_reputation.sql`), but no endpoint clears a bar, decrements a
   strike, or records a successful appeal. A buyer bar is permanent in the
   literal sense.
4. **Non-terminal stalling is invisible.** Because
   `TERMINAL_STATUSES_WITH_CONSEQUENCES` contains only the two failure
   statuses, a party that keeps a commitment in `Disputed` (or simply
   never triggers its own required action) never generates a reputation
   record. This is the link between this doc and
   [`dispute-stalling.md`](./dispute-stalling.md).

## 8. Conservation and the "nobody gets paid" checks

For every terminal status, ignoring gas and off-chain transfers:

- `cooperative_total_received = buyer_total_paid − buyer_total_refunded`,
  where the buyer's refunds include `reclaim_advance_*` and the `settle`
  refund. No stroop is created or destroyed; every path is a transfer
  from the contract's own balance or from the buyer's wallet.
- **No status pays the warehouse operator.** Not `Settled`, not any
  failure path. If the operator is to be paid, it is a separate transfer
  outside this contract, and it is not conditioned on anything the
  contract records.
- **No status pays an address that is not one of the three named
  parties.** Even the permissionless functions send funds only to
  `cooperative` or `buyer`, never to `msg.sender`. A third-party caller
  can choose *when* a sweep happens, but never *to whom*.
- **A tranche that is neither claimed nor expired blocks `settle`
  forever** (`TrancheUnresolved`) but does **not** block either default
  sweep or `reclaim_on_nondelivery`. That asymmetry is the reason several
  scenarios above turn on timing rather than on the settlement formula.

## 9. What this doc deliberately does not model

- **The physical crop, its grade, and its price off-chain.** The contract
  sees a `settlement_bps` and a token balance. A buyer who receives
  nothing but pays a small `settlement_bps` has still lost the crop.
- **The operator's own fee and liability.** Not in the contract, not in
  the reputation schema.
- **Gas and time value of money.** Real, sometimes decisive (see the
  `Cancelled` vs. `Forfeited` comparison in §6.1), but not denominated
  in the settlement token.
- **Off-chain legal recourse.** The contract is the whole of the on-chain
  remedy; anything beyond it is contract law and the operator's appeals
  process, neither of which is represented in this repo.
