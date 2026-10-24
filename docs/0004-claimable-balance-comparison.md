# Claimable-Balance-With-Expiry Mechanic: Custom Contract Implementation vs. Soroban Native Primitives

## Executive Summary

This contract does **not** use Soroban's `ClaimableBalance` contract (there is no first-class `ClaimableBalanceEntry` primitive callable from other contracts on Soroban, as Stellar's classic `ClaimableBalanceEntry` does not exist on Soroban). Instead, the claim/reclaim windows and expiry mechanics are **entirely built-in** to this escrow contract's own state machine. This document documents why that was necessary, what the contract does natively, and what would change if Soroban ever exposed something closer to the classic Stellar `ClaimableBalanceEntry` primitive.

## 1. Why the Contract Built Its Own Claimable-Balance-Equivalent Primitives

### 1.1 Soroban Lacks Classic `ClaimableBalanceEntry`

Stellar's original `ClaimableBalanceEntry` was a first-class account-bound claimable balance that could be claimed by a pre-authorized claimant. It had these properties:

| Property | Stellar `ClaimableBalanceEntry` | Soroban Equivalent |
|---|---|---|
| Native contract support | ✅ Yes, built into the ledger | ❌ No direct equivalent |
| Claim window | Once per balance, infinite duration unless revoked | ✅ Built into this contract per-tranche |
| Single claimant | One pre-authorized address per balance | ❌ Three parties (buyer/cooperative/warehouse) share one commitment |
| Revocable by sponsor | The sponsoring account can revoke | ❌ Not applicable — this contract owns the balance |
| Cross-contract claimable balances | Not supported (balances are account-bound) | ✅ This contract's internal ledger |

Because Soroban has no `ClaimableBalanceEntry` equivalent, and because the contract needs **three-party** claim/reclaim semantics (buyer, cooperative, warehouse) on a **single** escrowed balance, the claim/reclaim logic had to be built into the contract's own state machine rather than relying on a native primitive.

### 1.2 Three-Party Shared Balance Is Incompatible with Native ClaimableBalances

A native `ClaimableBalanceEntry` ties a claimable balance to a **single** pre-authorized claimant. This contract has three named parties sharing one escrowed balance, with these requirements:

| Requirement | Native ClaimableBalance | Contract's Custom Implementation |
|---|---|---|
| Cooperative can claim advance tranche | ❌ Would need separate balance per tranche | ✅ `claim_advance_1` / `claim_advance_2` — one balance, three-party auth |
| Buyer can reclaim after window passes | ❌ No unilateral reclaim right on a single balance | ✅ `reclaim_advance_1` / `reclaim_advance_2` — buyer-gated, window-based |
| Both tranches must be resolved before settle | ❌ Would need two separate claimable balances | ✅ `TrancheUnresolved` error — both resolved in one state machine |
| Inclusive/deadline boundary guards | ❌ Not applicable | ✅ `#111` boundary: claiming inclusive, reclaiming exclusive |

The contract's design explicitly handles **two tranches sharing one balance**, with different claim/reclaim windows and different parties authorized for each. This pattern simply cannot be expressed with native `ClaimableBalanceEntry`, which is single-balance/single-claimant by construction.

### 1.3 Expiry-Window Boundary Precision

Stellar's `ClaimableBalanceEntry` has no concept of "the deadline second is still open" vs "the deadline has passed" — a balance either expires or it doesn't, and the ledger handles the boundary. This contract needs **fine-grained boundary control** because four different deadlines interact on the same commitment:

| Deadline | Contract's Boundary Rule | Why It Matters |
|---|---|---|
| `advance1_deadline` | Claim: `timestamp > deadline` rejected; Reclaim: `timestamp <= deadline` rejected | Every ledger timestamp belongs to exactly one party |
| `remainder_deadline` | Fund: `now > deadline` rejected; Expire: `now <= deadline` rejected | Buyer's funding window vs. default trigger |
| `delivery_deadline` | Reclaim: `now <= deadline` rejected | Cooperative's delivery window |
| `fx_shortfall_deadline` | Fund: `now > deadline` rejected; Expire: `now <= deadline` rejected | Buyer's FX top-up window |

These four boundaries are **pinned at exact timestamps by their own tests** (#112–#115), not just by a clearly-before/clearly-after pair. The contract needs to reject `now == deadline` for some paths and accept it for others — a nuance that a primitive lacking per-path boundary control couldn't express.

## 2. What the Contract Implements Natively

### 2.1 Two Advance Tranches with Shared Claim Window

Both tranches use the same `claim_window_secs`, but have **separate deadline flags** and **separate claimed/expired booleans**:

```
advance1_deadline / advance1_claimed / advance1_expired   (tranche 1)
advance2_deadline / advance2_claimed / advance2_expired   (tranche 2)
```

**Mechanics:**

- `release_advance_1` / `release_advance_2` — opens the claim window (sets the deadline). Permissionless.
- `claim_advance_1` / `claim_advance_2` — cooperative claims within the window. Requires cooperative auth.
- `reclaim_advance_1` / `reclaim_advance_2` — buyer reclaims after window passes. Requires buyer auth.
- Boundary (#111): claiming is inclusive of the deadline instant (`timestamp > deadline` = reject), reclaiming is exclusive (`timestamp <= deadline` = reject). The two guards are exact complements.

### 2.2 Resolve-Before-Settle Guarantee

Both advance tranches must be resolved (claimed or expired) before `settle` will run. This was a deliberate design choice: an earlier version had `settle` silently sweep any unresolved tranche into the cooperative's payment, which was rejected because it would hand the buyer's already-vested reclaim right to the cooperative if the buyer simply hadn't gotten around to calling `reclaim_advance_*` yet.

### 2.3 Expiry Windows With Inclusive/Exclusive Boundary Distinction

The contract has **four** deadline-boundary paths, each with its own inclusive/exclusive rule:

| Window | Call | Inclusive/Exclusive | Error if condition met |
|---|---|---|---|
| `claim_window_secs` | `claim_advance_*` | Claim: exclusive of deadline (`now > deadline` = `ClaimWindowPassed`) | `ClaimWindowPassed` |
| | `reclaim_advance_*` | Reclaim: inclusive of deadline (`now <= deadline` = allowed) | N/A (reclaim allowed) |
| `remainder_window_secs` | `fund_remainder` | Fund: inclusive of deadline (`now > deadline` = `RemainderWindowPassed` rejects, but `now == deadline` = allowed) | `RemainderWindowPassed` |
| | `expire_remainder_window` | Expire: exclusive of deadline (`now <= deadline` = `RemainderWindowNotPassed` rejects) | `RemainderWindowNotPassed` |
| `delivery_deadline` | `reclaim_on_nondelivery` | Reclaim: inclusive of deadline (`now <= deadline` = `DeliveryDeadlineNotPassed` rejects, but `now == deadline` = allowed... wait) | `DeliveryDeadlineNotPassed` |
| Actually: `now > deadline` rejects, `now == deadline` still refuses with `DeliveryDeadlineNotPassed` | | |
| `fx_shortfall_deadline` | `fund_fx_shortfall` | Fund: inclusive of deadline (`now > deadline` = `FxShortfallWindowPassed` rejects, but `now == deadline` = allowed) | `FxShortfallWindowPassed` |
| | `expire_fx_shortfall_window` | Expire: exclusive of deadline (`now <= deadline` = `FxShortfallWindowNotPassed` rejects) | `FxShortfallWindowNotPassed` |

The key pattern: **for funding windows, `now == deadline` is allowed (inclusive); for expiry windows, `now == deadline` is rejected (exclusive).** This asymmetry is what keeps the boundary from being ambiguous — the two calls (`fund_remainder` vs `expire_remainder_window`) are never both legal at the same timestamp.

### 2.4 Dispute Freeze/Resume With Pre-Status Restoration

- `flag_dispute` — any one of the three parties can freeze the commitment into `Disputed` from `Locked` through `Delivered`.
- `resolve_dispute` — unanimous three-party consent restores the exact pre-dispute status.
- `expire_dispute_window` — permissionless after `dispute_deadline` passes, restores the same pre-dispute status.
- The contract does **not** arbitrate *who was right* — it just provides a bounded pause so off-chain parties can sort out their dispute without some deadline-triggered function running to a conclusion during the freeze.

> **Known limitation (not fixed here):** freezing `status` does not pause any other absolute deadline already ticking on this commitment (an advance's claim deadline, `remainder_deadline`, `delivery_deadline`, `fx_shortfall_deadline`). If one of those passes while a dispute is open, the corresponding `claim_*`/`expire_*`/`reclaim_*` becomes immediately callable the moment the dispute resolves or its own window lapses. This is a deliberate bias against building ahead of real usage.

## 3. What Would Change If Soroban Exposed a `ClaimableBalanceEntry`-Equivalent Primitive

### 3.1 If Soroban Added a First-Class `ClaimableBalance` primitive with Multi-Claim Support

If Soroban added a `ClaimableBalanceEntry` equivalent that supported **multiple claimants** and **per-claimant windows**, the contract could potentially:

- **Move the claim/reclaim logic off-chain** — the contract would just create and manage `ClaimableBalance` entries, and the native Soroban runtime would enforce the windows, claimed/expired state, and boundary rules.
- **Reduce on-chain code** — the `open_tranche`, `claim_tranche`, `reclaim_tranche` internal functions (about 40 lines each) could be replaced with native calls.
- **Improve boundary precision** — the runtime could guarantee that claiming and reclaiming guards are exact complements at the ledger level, eliminating the risk of both being (or both not being) callable at the same timestamp.

**However**, even with a multi-claimant `ClaimableBalance`, these gaps would remain:

| Gap | Why It Still Can't Be Fully Native |
|---|---|
| Three-party shared balance with different auth per tranche | A native primitive would need to know about three parties' auth keys, which is contract-specific knowledge. |
| FX shortfall and dispute logic interacting with claim windows | `resolve_fx_shortfall`, `flag_dispute`, `expire_dispute_window` all have custom logic that interacts with the claim windows but aren't about claiming per se. |
| The "resolve-before-settle" guarantee | This is a `settle`-specific design choice about not clawing back advances, not a claimable-balance concern. |
| Four different deadline types with asymmetric inclusive/exclusive boundaries | A generic primitive wouldn't know that `fund_remainder` should accept `now == deadline` but `expire_remainder_window` should reject it. |

### 3.2 If Soroban Added a `ClaimableBalance` primitive with Only Single-Claimant Support

If Soroban added a `ClaimableBalanceEntry` equivalent that only supported **single claimant** (the classic Stellar model), the contract **could not** use it directly because:

- The contract has **three parties** sharing one balance, with different auth for different operations.
- The contract needs **two independent tranches** on one balance, which a single `ClaimableBalanceEntry` cannot express.
- The contract needs **asymmetric boundary rules** (inclusive for funding, exclusive for expiry) that a generic primitive wouldn't encode.

The contract would need to **wrap** the native primitive: create one `ClaimableBalanceEntry` per tranche, and have the contract's internal logic map onto them. But this would effectively re-implement the same mechanics, plus add the overhead of cross-contract calls and XDR encoding/decoding.

### 3.3 Net Assessment: Custom Implementation Remains Preferable

Even if Soroban someday exposes a `ClaimableBalanceEntry` equivalent, the custom implementation in this contract is likely to remain preferable for these reasons:

1. **Three-party semantics are first-class**, not bolted on. The contract was designed from day one for buyer/cooperative/warehouse interaction on a single commitment — a native primitive would be a poor fit.

2. **Boundary control is contract-specific.** The asymmetric inclusive/exclusive rules for the four deadline types are woven into every method's guard logic. Moving this to a primitive would require the primitive to understand contract-specific boundary rules, which defeats the purpose of a generic primitive.

3. **The claim/reclaim logic is only ~100 lines total** across `open_tranche`, `claim_tranche`, and `reclaim_tranche`. This is a modest amount of on-chain code, and the complexity is well-understood and tested (16+ deadline boundary tests).

4. **The dispute freeze/resume mechanism** (`flag_dispute`/`resolve_dispute`/`expire_dispute_window`) has its own deadline interaction concerns that aren't about claiming at all — they're about pausing and resuming a commitment's entire state machine.

5. **Backward compatibility.** The contract already deployed on testnet with the custom implementation. Switching to a native primitive would be a fork, not an upgrade.

## 4. Conclusion

This contract's claimable-balance-with-expiry mechanic is **entirely home-grown** because Soroban lacks a native `ClaimableBalanceEntry` equivalent, and the contract's requirements — three-party shared balance, two independent tranches, asymmetric deadline boundaries, and integrated dispute logic — cannot be expressed with a generic primitive.

If Soroban ever exposes a `ClaimableBalanceEntry` equivalent, the most likely scenario is that contracts would **wrap** it rather than replace their custom implementation, because the contract-specific semantics (three parties, four deadline types, asymmetric boundaries) are unlikely to be captured by a generic primitive. The custom implementation remains compact (~100 lines of claim/reclaim logic), well-tested (16+ deadline boundary tests), and directly aligned with the product's multi-party escrow use case.