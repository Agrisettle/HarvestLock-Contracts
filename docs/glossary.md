# Glossary — HarvestLock escrow contract

A quick-reference translation of every `Status` and `Error` variant into
plain English. If a failed transaction gives you an error code, find it
below. If you want to know what state a commitment is in, find the status
word below. Source of truth is always `contracts/escrow/src/lib.rs`; this
file explains what the code means, not what it does.

---

## Status variants

| Variant | What it means |
|---|---|
| `Draft` | The commitment has been initialized but no money has been moved yet — the buyer has agreed to terms but not yet locked in. |
| `Locked` | The buyer's deposit (advance-tranche portion of the total) is held in escrow and the first advance tranche window has opened, or will open soon. |
| `Advance1Released` | The first advance tranche window is open; the cooperative can claim it, or the buyer can reclaim it once the window passes. |
| `CheckpointPassed` | The warehouse operator has attested a mid-season checkpoint, confirming the crop is on track; the second advance tranche window is now open. |
| `Advance2Released` | The second advance tranche window is open; same claim / reclaim mechanics as `Advance1Released`. |
| `ReadyForDelivery` | The cooperative has signaled it is ready to deliver; the buyer's remainder-payment window is now open and ticking. |
| `Delivered` | The warehouse operator has attested delivery and recorded a quantity and grade; the settlement path is now open. |
| `Settled` | Settlement has completed; all escrow funds have been distributed to the cooperative and/or refunded to the buyer. The commitment is finished. |
| `Cancelled` | Both the buyer and the cooperative agreed to unwind the deal; any escrowed funds were returned to the buyer. |
| `Defaulted` | The buyer failed to fund the remainder before its deadline, **or** failed to pay an FX shortfall before its deadline; escrowed funds were swept to the cooperative. |
| `Forfeited` | The cooperative never reached `Delivered` before the delivery deadline; escrowed funds were returned to the buyer. |
| `Disputed` | One of the three named parties froze the commitment; every other state-changing call is blocked until the freeze is lifted by unanimous consent or its deadline passes. |

---

## Error variants

| Code | Variant | What it means in plain English |
|---|---|---|
| 1 | `AlreadyInitialized` | `initialize` has already been called on this contract instance; it can only run once. |
| 2 | `NotInitialized` | `initialize` has not run yet; nothing else can happen until it does. |
| 3 | `InvalidState` | The commitment is not in the status this call requires — for example, calling `lock` when the status is already `Locked`. |
| 4 | `InvalidBps` | The advance-tranche percentages (`advance1_bps + advance2_bps`) add up to more than 100 %; a deposit cannot exceed the full contract value. |
| 5 | `ZeroAmount` | `total_amount` was zero or negative; a commitment must have a positive value. |
| 6 | `InvalidWindow` | One of the time-window parameters (`claim_window_secs`, `remainder_window_secs`, or `delivery_window_secs`) was zero; every window must be at least one second. |
| 7 | `NotYetOpened` | The advance tranche has not been opened yet (the corresponding `release_advance_*` call has not run). |
| 8 | `AlreadyClaimed` | The advance tranche was already claimed by the cooperative; it cannot be claimed again. |
| 9 | `AlreadyExpired` | The advance tranche was already returned to the buyer via `reclaim_advance_*`; it cannot be reclaimed again. |
| 10 | `ClaimWindowPassed` | The cooperative tried to claim an advance after its window closed; only the buyer can reclaim it now. |
| 11 | `ClaimWindowNotPassed` | The buyer tried to reclaim an advance before its window closed; the cooperative still has time to claim. |
| 12 | `TrancheUnresolved` | `settle` (or `resolve_fx_shortfall`) requires both advance tranches to be either claimed or reclaimed before it will run; at least one is still open. |
| 13 | `RemainderAlreadyFunded` | The buyer already funded the remainder; it cannot be funded twice. |
| 14 | `RemainderWindowPassed` | The buyer tried to fund the remainder after the deadline; `expire_remainder_window` is the only legal path from here. |
| 15 | `RemainderWindowNotPassed` | `expire_remainder_window` was called before the deadline; the buyer's payment window is still open. |
| 16 | `RemainderNotFunded` | `confirm_delivery` was called but the buyer has not funded the remainder yet; delivery cannot be confirmed while money is still owed. |
| 17 | `DeliveryDeadlineNotPassed` | `reclaim_on_nondelivery` was called before the delivery deadline; the cooperative still has time to deliver. |
| 18 | `InvalidQuantity` | `contracted_quantity` was zero at `initialize`; a contract must specify a positive quantity. |
| 19 | `InvalidGradeSchedule` | `grade_price_bps` was empty, or one of its entries exceeded 10,000 bps (100 %); a grade tier cannot be worth more than the full agreed unit price. |
| 20 | `InvalidGradeIndex` | `confirm_delivery`'s `grade_index` does not exist in `grade_price_bps`; check the grade schedule. |
| 21 | `AllocationAlreadySet` | `set_allocation` has already run once; the allocation ledger is immutable after it is recorded. |
| 22 | `InvalidAllocation` | The member list was empty, or the sum of all `share_bps` values exceeded 10,000 bps (100 %). |
| 23 | `AllocationNotSet` | `get_allocation` was called before `set_allocation` has ever run; the allocation ledger does not exist yet. |
| 24 | `InvalidOracleConfig` | `oracle_config.max_age_secs` was zero; a staleness bound of zero would reject every real oracle quote. Use `None` for a deal with no oracle. |
| 25 | `OracleNotConfigured` | The call requires a Reflector oracle, but `initialize` was given `oracle_config: None`; this commitment has no FX conversion path. |
| 26 | `OraclePriceUnavailable` | The configured Reflector oracle has no quote for the requested asset; check that the oracle instance supports the asset symbol in `oracle_config.price_asset`. |
| 27 | `OracleStale` | The most recent oracle quote is older than `oracle_config.max_age_secs` allows; wait for Reflector to publish a fresh quote and retry. |
| 28 | `FxAlreadyResolved` | `resolve_fx_shortfall` has already run once; re-resolving at a different rate is not allowed. |
| 29 | `FxNotResolved` | `fund_fx_shortfall`, `expire_fx_shortfall_window`, or `settle` was called before `resolve_fx_shortfall` has run on an oracle-configured commitment. |
| 30 | `FxShortfallUnfunded` | `settle` was called but the FX shortfall computed by `resolve_fx_shortfall` has not been paid in yet. |
| 31 | `NoFxShortfall` | `fund_fx_shortfall` or `expire_fx_shortfall_window` was called, but `resolve_fx_shortfall` found no shortfall — the escrowed amount already covered the obligation; `settle` can run directly. |
| 32 | `FxShortfallAlreadyFunded` | The buyer already paid the FX shortfall; it cannot be funded again. |
| 33 | `FxShortfallWindowPassed` | `fund_fx_shortfall` was called after the deadline; `expire_fx_shortfall_window` is the only legal path from here. |
| 34 | `FxShortfallWindowNotPassed` | `expire_fx_shortfall_window` was called before the deadline; the buyer's payment window is still open. |
| 35 | `NotAParty` | `flag_dispute` was called with a `flagger` address that is not the buyer, cooperative, or warehouse operator; only a named party can flag a dispute. |
| 36 | `DisputeWindowNotPassed` | `expire_dispute_window` was called before the dispute deadline; the freeze is still in effect. |
| 37 | `GradeScheduleTooLong` | `grade_price_bps` had more than `MAX_GRADE_ENTRIES` (16) entries; that is longer than any realistic grading schedule. |
| 38 | `AllocationTooLarge` | `set_allocation`'s member list had more than `MAX_ALLOCATION_MEMBERS` (200) entries; that exceeds the PRD's cooperative scale. |
| 39 | `AmountTooLarge` | `total_amount` exceeded `MAX_TOTAL_AMOUNT` (`i128::MAX / 10_000`); at that scale the internal basis-point arithmetic would overflow. |
| 40 | `OraclePriceInvalid` | The oracle returned a zero or negative price; a non-positive exchange rate cannot produce a valid settlement amount. |
| 41 | `DeliveryGraceWindowNotPassed` | `expire_delivery_window` was called before the grace period (`delivery_deadline + remainder_window_secs`) has strictly passed. |
| 42 | `DisputeCapReached` | The commitment has already accumulated `MAX_DISPUTE_FLAGS` (3) lapsed dispute windows; `flag_dispute` will not start another. |
| 43 | `DeliveryDeadlinePassed` | `confirm_delivery` was called after the delivery deadline; `reclaim_on_nondelivery` is the only legal path from here. |
| 44 | `OracleConversionOutOfRange` | The oracle conversion's numerator or decimal divisor cannot be represented as an `i128`; settlement refuses rather than trapping on an unusable oracle value. |
| 45 | `OracleTimestampInFuture` | The configured Reflector oracle returned a quote timestamp later than the current ledger timestamp; wait for a valid quote. |
| 46 | `OracleUnavailable` | The configured oracle contract could not answer at all — no contract at that address, a callee that panicked or trapped, or a reply that would not decode. Distinct from `OraclePriceUnavailable`, which means it answered and quotes no such asset. |
| 47 | `LockAfterDeliveryDeadline` | `lock` was called after `delivery_deadline` had strictly passed. The delivery window starts at `initialize`, so a late lock would hand the buyer an immediate `reclaim_on_nondelivery` against a cooperative that never had time to deliver. |
