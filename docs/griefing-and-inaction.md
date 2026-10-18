# Griefing by inaction — what happens when a party simply does nothing

Issue #116 asked, for each required action, what economic recourse the
other parties have today, and whether any scenario leaves funds genuinely
stuck with no path forward for anyone. This doc walks every required
action, then the four scenarios where inaction is not merely annoying but
value-destroying or profit-maximizing.

Ground truth is `contracts/escrow/src/lib.rs` and, for the off-chain
layer, `api/src/reputation.ts` in the sibling `HarvestLock` repo.
Companion docs: [`terminal-outcomes.md`](./terminal-outcomes.md) (what
each status pays), [`dispute-stalling.md`](./dispute-stalling.md)
(weaponized delay), [`dispute-frontrunning.md`](./dispute-frontrunning.md)
(races created by the missing guards).

## TL;DR

- **Most inaction has a recourse, but the recourse is usually "the harmed
  party calls a function later", not "the contract protects them".**
  There is no keeper, no bounty, and no permissionless rescue for the two
  paths that matter most (`reclaim_on_nondelivery` is buyer-gated;
  `confirm_delivery` has no substitute).
- **One scenario is genuinely stuck: the cooperative never delivers and
  the buyer never calls `reclaim_on_nondelivery`.** The contract then
  holds the escrow forever. No third party, not even the operator, can
  move it. The only unilateral exit is the buyer's, and it is the buyer
  who benefits from using it — so this needs *both* the cooperative's
  non-performance and the buyer's inaction (or disappearance).
- **Two near-stuck scenarios are worse than they look because the
  griefer pays nothing:** a cooperative that never signals
  `ready_for_delivery`, and an operator that never marks the checkpoint
  or confirms delivery. Both can lock the buyer's deposit for the whole
  `delivery_window_secs` — up to 365 days through the API — and neither
  generates a strike, a bar, or any other record while it happens.
- **The warehouse operator is the only party with no on-chain payout and
  no off-chain consequence.** It can never take funds, but it can stop
  the deal indefinitely, attest late, attest a zero quantity, or never
  attest, and nothing in this repo records or prices any of it.
- **Doing nothing is sometimes the profit-maximizing move**, not just
  spite: a buyer facing an FX shortfall keeps the goods and the top-up
  and only loses a reputation record it may not need (§4.1).
- **The off-chain bar is not a safety net for liveness.** It is applied
  only when someone reads a *terminal* status through the API, only at
  new-commitment creation, and only to the buyer or cooperative. It
  cannot get funds out of a stuck contract.

## 1. Every required action, and who can unstick it

`T = 1,000,000`; advances 15% + 20% (`D = 350,000`); remainder
`R = 650,000`; delivery window 120 days. "Griefer cost" means what the
party doing nothing loses by doing nothing.

| Required action | Who must act | What inaction blocks | Who can unstick, and how | Griefer cost | Verdict |
| --- | --- | --- | --- | --- | --- |
| `lock` (fund the deposit) | Buyer | Everything; the deal never starts | Nobody. Nothing is escrowed, so nothing is lost | 0 | Safe — no funds at risk |
| `set_allocation` | Cooperative | Nothing fund-moving; ledger is optional and record-only in v1 | Cooperative only; `lock` is not gated on it | 0 | Not a griefing surface |
| `release_advance_1/2` | **Anyone** | The tranche clock never starts | Anyone, immediately | — | Not a griefing surface |
| `claim_advance_1/2` | Cooperative | It forgoes its own money; after the window lapses the buyer can reclaim | Buyer via `reclaim_advance_*`; no third party needed | Forfeits the tranche (15–20% of `T`) | Buyer-protective; also the cooperative's own loss |
| `reclaim_advance_1/2` | Buyer | Its own vested refund sits in the contract | Buyer only; if it never acts, the money is swept to the cooperative on default or returned on forfeit/cancel | Forfeits the tranche on a default sweep | Buyer's own loss; see the race in `dispute-frontrunning.md` §3.2 |
| `mark_checkpoint` | Operator | Tranche 2 never opens; `ReadyForDelivery` unreachable | ***Nobody.*** The buyer's only exit is `reclaim_on_nondelivery` after `delivery_deadline` | 0; no record | **Griefing hole** (§4.3) |
| `ready_for_delivery` | Cooperative | Remainder is never requested; the deal stalls at `Advance2Released` | Nobody early. Buyer waits out `delivery_deadline`, then `reclaim_on_nondelivery`; or both sign `cancel` | 0; no record | **Griefing hole** (§4.2) |
| `fund_remainder` | Buyer | Delivery cannot be confirmed or settled | **Anyone** via `expire_remainder_window` after the window; 100% of the balance to the cooperative, buyer permanently barred | Loses the whole remaining escrow + bar | Complete remedy (§3.1) |
| `confirm_delivery` | Operator | `settle` never runs; the remainder is locked | Buyer after `delivery_deadline` via `reclaim_on_nondelivery`; or both sign `cancel` | 0; no record; can even attest late | **Griefing hole** (§4.3), plus the race in `dispute-frontrunning.md` §3.1 |
| `fund_fx_shortfall` | Buyer | `settle` never runs; escrow sits in `Delivered` | **Anyone** via `expire_fx_shortfall_window`; cooperative gets the escrow, buyer barred | Keeps the shortfall; loses the bar only | **Remedy is incomplete** (§3.2) |
| `resolve_dispute` | All three | The freeze continues | **Anyone** via `expire_dispute_window` after the window; status restored exactly | 0; re-flag possible | Bounded per flag, unbounded in aggregate (`dispute-stalling.md`) |
| `settle` | **Anyone** | Funds stay in `Delivered` | Anyone; normally the cooperative (it gets paid) or the buyer (it gets the refund) | — | Self-interested caller exists |
| `reclaim_on_nondelivery` | **Buyer only** | The escrow never returns; `Forfeited` is never recorded | ***Nobody else.*** No permissionless fallback exists | 0; no record unless the buyer acts | **The one genuinely stuck state** (§2) |
| `cancel` | Buyer + cooperative | No early unwind; a terminal path may still exist | Nobody unilaterally — by design | 0; no record | Mutual by definition; the rescue path for a dead operator (§4.4) |
| `reassign_buyer` | Current buyer + cooperative + incoming buyer | Irrelevant to funds | — | — | Not a griefing surface |

Two structural notes from the table:

- **Every permissionless function is unpriced.** There is no caller fee.
  For each one, either a party benefits directly (`settle`,
  `expire_remainder_window`, `expire_fx_shortfall_window` all pay the
  cooperative; `release_advance_*` starts the cooperative's clock) or
  the harmed party calls it (`expire_dispute_window`). A third party
  never profits, so a third party never acts. Liveness is therefore
  assumed, not incentivized.
- **The two paths without a fallback are the two that protect the buyer
  and the buyer's exit, respectively**: `confirm_delivery` has no
  substitute (only the operator can attest), and
  `reclaim_on_nondelivery` is buyer-gated (only the buyer can reclaim).
  The asymmetry is deliberate — each is a judgment or a benefit owed to
  a specific party — but it means the contract's liveness depends on the
  operator and the buyer being present.

## 2. The genuinely stuck state: cooperative silent, buyer absent

Concrete: the buyer locks 350,000, the cooperative claims tranche 1
(150,000), then never delivers, never signals ready-for-delivery, and
never responds. The buyer's remainder is never funded, so the only
balance is 200,000.

After `delivery_deadline`:

- `reclaim_on_nondelivery` would return the 200,000 to the buyer — but
  **only the buyer can call it.** It requires `c.buyer.require_auth()`.
- `cancel` would return the 200,000 — but it requires the cooperative's
  signature too, which will not come.
- `expire_remainder_window` is not available: it needs
  `ReadyForDelivery`, which the cooperative never triggered.
- `settle`, `reclaim_advance_*` as a rescue, `expire_dispute_window`,
  `reassign_buyer` — none apply. `reassign_buyer` needs the current
  buyer, the cooperative, *and* the incoming buyer; it cannot be used to
  replace an absent cooperative.
- The operator has no function that can move the balance.

So the 200,000 sits in the contract indefinitely. The buyer is the only
party with a key to it, and the buyer is the party who has vanished. If
the buyer is merely slow, the funds are recoverable; if the buyer's key
is lost or the entity is gone, **there is no path for anyone**. The
cooperative cannot even clear the commitment: it keeps the 150,000 it
already claimed and the deal never reaches a terminal status, so
`api/src/reputation.ts` never records the non-delivery. The cooperative
that never delivered is not even struck.

This is the answer to the issue's "genuinely stuck" question. It is the
only state where value is locked with no unilateral or permissionless
exit, and it requires exactly the combination a real pilot can produce:
a seller who stops performing and a buyer who stops monitoring.

The mirror case — cooperative delivers, buyer disappears — is **not**
stuck. `expire_remainder_window` and `expire_fx_shortfall_window` are
permissionless, and even if the cooperative has to call them itself, it
gets paid. The asymmetry exists because the buyer's failure is
deadline-detectable while the seller's is not (the module doc's own
explanation for two-phase funding).

## 3. Buyer-default paths: complete for the remainder, incomplete for FX

### 3.1 Remainder default — the cooperative has a real remedy

Buyer funds the deposit (350,000), the cooperative advances the deal to
`ReadyForDelivery`, the buyer never funds the 650,000 remainder.

- Anyone calls `expire_remainder_window` after the window.
- The cooperative receives the whole 200,000 still in the contract (the
  two unclaimed advances), or 0 if it already claimed both.
- The buyer is permanently barred on observation.
- The cooperative's loss is the expected sale, not the escrow. It may
  even be whole if its early-season costs are below the 350,000 deposit.

This is the best-designed griefing remedy in the contract: deadline-
triggered, permissionless, destination fixed, and the penalty funds the
harmed party. Its only flaw is the sweep's interaction with lapsed but
unreclaimed tranches ([`dispute-frontrunning.md`](./dispute-frontrunning.md)
§3.2).

### 3.2 FX-shortfall default — the buyer can rationally refuse

State: `Delivered`, full delivery at full grade, escrow 1,000,000, fresh
rate makes the obligation 1,350,000, recorded shortfall 350,000,
deadline passes with no top-up.

- Anyone calls `expire_fx_shortfall_window`; the cooperative receives the
  650,000 remainder (plus its 350,000 claimed advances) = 1,000,000.
- The buyer keeps the 350,000 it did not pay and the goods the contract
  values at 1,350,000. Its cash position is **350,000 better than if it
  had performed**.
- Its only additional loss is the permanent bar.

The cooperative's remedy is "everything still escrowed", which is the
stale-rate value, not the obligation. The shortfall was never collateral.
The cooperative must also be the one to call the expiry, because no third
party is paid to and the buyer has no reason to. It will, since it gets
the escrow — but if the cooperative is the one that is also gone, the
funds sit. Again, liveness depends on an interested party, not on the
contract.

This is the clearest case where **inaction is profitable**: the buyer's
marginal decision after delivery is "pay 350,000 for no additional goods,
or keep it and accept a bar". The bar is the only deterrent, and it is
address-scoped and not enforced on-chain.

## 4. The griefing holes: operator and cooperative silence

### 4.1 The operator can never be replaced

There is no `reassign_operator`, no admin, no upgrade, and no fallback
attestation. `mark_checkpoint` and `confirm_delivery` are the operator's
alone. If the operator's key is lost, the deal can only end in:

- `Forfeited` (buyer calls `reclaim_on_nondelivery` after the deadline),
  or
- `Cancelled` (both remaining parties agree to unwind), or
- `Defaulted` (only if the remainder was never funded and the
  cooperative reaches `ReadyForDelivery` — which needs
  `mark_checkpoint`, so this is usually unreachable too).

The cooperative's early advances stay with it in every case, but the
remainder is lost. The operator suffers nothing: no fee is withheld, no
strike, no bar, no event type. Even a *present* operator can do this
without consequence by simply not calling the function, because there is
no deadline on `mark_checkpoint` or `confirm_delivery` and no service
level recorded on-chain.

### 4.2 The cooperative can hold the buyer's deposit hostage

Before `ready_for_delivery`, the buyer's only exits are:

- `cancel` — needs the cooperative's signature;
- `reclaim_on_nondelivery` — needs `delivery_deadline` to pass.

A cooperative that wants leverage (a price renegotiation, a side deal, or
simply to deny a competitor liquidity) can refuse to signal
`ready_for_delivery` and let the delivery clock run. The buyer's 350,000
is locked for up to `delivery_window_secs` — 120 days in the app default,
up to 365 days through the API — while the cooperative earns nothing but
pays nothing either. There is no penalty for the delay, no strike, and
no status that records it. The buyer's "remedy" is to wait and then take
its own deposit back via a forfeiture that also strikes the cooperative.

Compare the reverse: a buyer who stalls at `ReadyForDelivery` hands the
cooperative the entire balance. The contract punishes buyer delay and
not seller delay, because only the buyer's obligation has a deadline
that triggers a transfer to the counterparty.

### 4.3 The operator can under-attest, not just abstain

`confirm_delivery(delivered_quantity, grade_index)` is authoritative and
trusted as-is. An operator can attest `delivered_quantity = 0` (or
`grade_index` = the lowest entry) and drive `settlement_bps` to 0, after
which `settle` refunds the entire remainder to the buyer and the
cooperative gets only its claimed advances. It can also attest a full
quantity at the lowest grade. There is no receipt hash, no second
signature, and no on-chain appeal; the PRD assigns grade appeals to the
operator's own process, which is not implemented here. The operator's
incentive to do this is whatever the buyer is willing to pay off-chain —
and again, no consequence attaches to the operator itself.

### 4.4 Mutual cancellation is the only rescue from a dead counterparty key

`cancel` requires buyer and cooperative, so it cannot rescue a deal from
an absent cooperative. But it is the only path that unwinds a deal when
the **operator** is dead while the buyer and cooperative are both
present: it is legal from `ReadyForDelivery`, returns the entire balance
to the buyer, leaves claimed advances with the cooperative, and records
no forfeiture. Both sides are at least as well off as the `Forfeited`
outcome, and the cooperative avoids a strike. This is a real, useful
property — and it is also why the cooperative's hostage strategy in
§4.2 has a negotiated ending: the buyer can offer to cancel, and the
cooperative can accept without a penalty.

## 5. What "recourse" actually means here

| Scenario | Is there a path? | Who must act | Is the harmed party made whole? |
| --- | --- | --- | --- |
| Buyer never funds remainder | Yes | Anyone (`expire_remainder_window`) | Cooperative gets all escrow; sale is lost |
| Buyer never funds FX shortfall | Yes | Anyone (`expire_fx_shortfall_window`) | Cooperative gets stale-rate value; shortfall lost |
| Cooperative never claims advances | Yes | Buyer (`reclaim_advance_*`) | Buyer recovers the tranche after the window |
| Cooperative never delivers, buyer present | Yes | Buyer (`reclaim_on_nondelivery`) | Buyer recovers the balance; strike recorded |
| Cooperative never delivers, buyer absent | **No** | Nobody | **Funds stuck; no strike recorded** |
| Cooperative never signals ready-for-delivery | Yes, but late | Buyer after `delivery_deadline`, or both via `cancel` | Deposit locked for the whole delivery window; no penalty |
| Operator never marks checkpoint | Yes, but late | Buyer after `delivery_deadline` | Deposit locked; no penalty for the operator |
| Operator never confirms delivery | Yes, but late | Buyer after `delivery_deadline`; or both via `cancel` | Remainder locked; no penalty for the operator |
| Operator attests zero quantity | **No** | Nobody | Cooperative loses the remainder; no appeal on-chain |
| Dispute never resolved | Yes | Anyone (`expire_dispute_window`) | Status restored; delay not compensated |
| Nobody calls `settle` | Yes, if there is money to move | Cooperative or buyer (self-interest) | Fine; no bounty needed |

The pattern is consistent: **the contract's liveness is provided by the
party who benefits, not by the protocol.** Where the beneficiary is
absent, or where the only actor is the party who caused the problem,
there is no rescue. Only one combination leaves funds with no path at
all, but several leave a harmed party waiting out a window with no
compensation.

## 6. Off-chain reputation does not close the gap

`api/src/reputation.ts` records consequences only for a fresh transition
into `Defaulted` or `Forfeited`, when the API observes it:

- the cooperative's §2 walk-away records **nothing**, because no terminal
  status is ever reached;
- the operator's §4.1/§4.3 behaviour records **nothing**, because there
  is no operator event type;
- a party that stalls in `Disputed` or simply leaves a commitment in
  `Locked` records **nothing**, because there is no transition;
- a `Defaulted` the API never reads (no `GET`, no submission with
  `refreshContractId`) records **nothing** — the recording is a
  best-effort cache side effect, not an event listener;
- the bar is enforced only when creating a new commitment through the
  API, only for the buyer and cooperative, and not for a buyer arriving
  via `reassign_buyer`.

So the reputation layer is a deterrent for the narrow, observable cases
(buyer remainder default, cooperative non-delivery that the buyer
actually reports) and a non-factor for precisely the inaction cases this
issue is about. See [`terminal-outcomes.md`](./terminal-outcomes.md) §7
for the full matrix.

## 7. What would close the holes (product decisions, not implemented here)

Listed so the gaps are explicit rather than implied, roughly in
value-for-complexity order:

1. **A permissionless forfeiture fallback** with a grace period after
   `delivery_deadline` (destination fixed to the buyer, status
   `Forfeited`), so §2 cannot happen. This also removes the buyer-gated
   liveness assumption.
2. **A `delivery_deadline` guard on `confirm_delivery`**, so late
   delivery cannot beat the forfeiture right — the headline finding of
   [`dispute-frontrunning.md`](./dispute-frontrunning.md), and it also
   narrows the cooperative's §4.2 leverage.
3. **Require resolved tranches before either expiry sweep**, mirroring
   `settle`'s `TrancheUnresolved` guard, so a sweep cannot eat a vested
   reclaim right.
4. **An operator replacement or attestation-expiry path** (a
   three-party-consented `reassign_operator`, or a bounded attestation
   window after which the buyer's forfeiture is exclusive). The contract
   currently has no way to recover from a lost operator key.
5. **A caller bounty on permissionless terminal calls** (a few bps of
   the swept balance to the caller), so liveness does not depend on the
   beneficiary being present and solvent. This needs care: a bounty also
   creates an incentive to trigger sweeps early, which is exactly the
   race in §3.2 — it should be paired with fix 3.
6. **Record the non-terminal failure modes off-chain** (operator
   silence, repeated flags, abandoned commitments) if the product wants
   reputation to cover them. That is an API/product change, not a
   contract one, and `standing_events` already has the shape for it.

None of these are implemented in this PR — this issue asked for the
analysis, and the analysis is that the contract is safe against theft by
inaction but not against **value being locked or delayed** by it.

This extends [`threat-model.md`](./threat-model.md)'s trust assumption 2
("the warehouse operator is honest") with what happens when it is simply
*absent* rather than dishonest, and its stalling verdict 16 with the
griefing paths that need no dispute at all.
