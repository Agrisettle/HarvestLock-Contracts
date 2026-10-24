# HarvestLock Escrow Contract — Machine-Readable Interface Specification

This document provides a machine-consumable specification of all public methods on the `EscrowContract` in the HarvestLock escrow contract (Soroban). The format is JSON with a human-readable markdown preface.

## Specification Format

```json
{
  "contract": "HarvestLockEscrow",
  "version": "0.1.0",
  "methods": [
    { ... },  // one entry per public method
    ...
  ]
}
```

Each method entry has the following fields:

- **name** (string): The function name as it appears in the contract.
- **args** (array of objects): Each argument has a `name` and `type`. Types follow the Rust/Soroban type system as documented in the contract's `#[contracterror]` and type definitions.
- **returns** (string): The primary return type as a Rust type name (e.g., `Result<(), Error>`, `Status`, `Vec<AllocationMember>`).
- **errors** (array of strings): The `Error` variants that this method can return, listed by their constant name (e.g., `["InvalidState", "RemainderNotFunded"]`). Omitted if the method never returns an `Error` (only `Ok`).
- **state_transition** (object, optional): The from→to status transition this method enables, if applicable. Fields: `from` (Status), `to` (Status).
- **auth_requirement** (string, optional): Which party must authorize the call, or "permissionless" if no auth is required.

---

## Method Specifications

### 1. `initialize`

```json
{
  "name": "initialize",
  "args": [
    { "name": "buyer", "type": "Address" },
    { "name": "cooperative", "type": "Address" },
    { "name": "warehouse_operator", "type": "Address" },
    { "name": "token", "type": "Address" },
    { "name": "total_amount", "type": "i128" },
    { "name": "advance1_bps", "type": "u32" },
    { "name": "advance2_bps", "type": "u32" },
    { "name": "claim_window_secs", "type": "u64" },
    { "name": "remainder_window_secs", "type": "u64" },
    { "name": "delivery_window_secs", "type": "u64" },
    { "name": "contracted_quantity", "type": "u32" },
    { "name": "grade_price_bps", "type": "Vec<u32>" },
    { "name": "oracle_config", "type": "Option<OracleConfig>" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["AlreadyInitialized", "ZeroAmount", "InvalidBps", "InvalidWindow", "InvalidQuantity", "InvalidGradeSchedule", "GradeScheduleTooLong", "InvalidOracleConfig"],
  "state_transition": { "from": "Draft", "to": "Locked" },
  "auth_requirement": "buyer"
}
```

*Creates the commitment in `Draft`. No funds move yet. Requires the buyer's auth.*

### 2. `get_oracle_config`

```json
{
  "name": "get_oracle_config",
  "args": [],
  "returns": "Result<Option<OracleConfig>, Error>",
  "errors": ["NotInitialized"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — read-only view, callable by anyone.*

### 3. `oracle_rate`

```json
{
  "name": "oracle_rate",
  "args": [],
  "returns": "Result<OracleRate, Error>",
  "errors": ["OracleNotConfigured", "OraclePriceUnavailable", "OracleStale"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*Reads the current rate for this commitment's configured oracle asset, enforcing the staleness bound. Pure read against Reflector's own stored history, not a state transition.*

### 4. `set_allocation`

```json
{
  "name": "set_allocation",
  "args": [
    { "name": "members", "type": "Vec<AllocationMember>" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "AllocationAlreadySet", "InvalidAllocation", "AllocationTooLarge"],
  "state_transition": { "from": "Draft", "to": "Draft" },
  "auth_requirement": "cooperative"
}
```

*Cooperative-authored record of member farmers' entitlement shares — PRD's "member allocation ledger capture at lock-in." One-time and immutable once set. Requires cooperative auth.*

### 5. `get_allocation`

```json
{
  "name": "get_allocation",
  "args": [],
  "returns": "Result<Vec<AllocationMember>, Error>",
  "errors": ["AllocationNotSet"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — read-only view, callable by anyone.*

### 6. `lock`

```json
{
  "name": "lock",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "ZeroAmount", "InvalidBps"],
  "state_transition": { "from": "Draft", "to": "Locked" },
  "auth_requirement": "buyer"
}
```

*Draft → Locked. Pulls the buyer's deposit into the contract — `advance1_bps + advance2_bps` of `total_amount`, not the full amount. No prior SEP-41 `approve()` needed.*

### 7. `release_advance_1`

```json
{
  "name": "release_advance_1",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "Locked", "to": "Advance1Released" },
  "auth_requirement": "permissionless"
}
```

*Locked → Advance1Released. Opens tranche 1's claim window — does not move funds. Deliberately not auth-gated.*

### 8. `claim_advance_1`

```json
{
  "name": "claim_advance_1",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "NotYetOpened", "AlreadyClaimed", "AlreadyExpired", "ClaimWindowPassed"],
  "state_transition": null,
  "auth_requirement": "cooperative"
}
```

*Cooperative claims tranche 1, if within the window. Boundary (#111): "within" is inclusive of the deadline instant.*

### 9. `reclaim_advance_1`

```json
{
  "name": "reclaim_advance_1",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "NotYetOpened", "AlreadyClaimed", "AlreadyExpired", "ClaimWindowNotPassed"],
  "state_transition": null,
  "auth_requirement": "buyer"
}
```

*Buyer reclaims tranche 1, if the window has passed unclaimed. Boundary (#111): the exact complement of `claim_advance_1`.*

### 10. `mark_checkpoint`

```json
{
  "name": "mark_checkpoint",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "Advance1Released", "to": "CheckpointPassed" },
  "auth_requirement": "warehouse_operator"
}
```

*Advance1Released → CheckpointPassed. Requires the warehouse operator's attestation.*

### 11. `release_advance_2`

```json
{
  "name": "release_advance_2",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "CheckpointPassed", "to": "Advance2Released" },
  "auth_requirement": "permissionless"
}
```

*CheckpointPassed → Advance2Released. Same mechanics as `release_advance_1`, for tranche 2.*

### 12. `claim_advance_2`

```json
{
  "name": "claim_advance_2",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "NotYetOpened", "AlreadyClaimed", "AlreadyExpired", "ClaimWindowPassed"],
  "state_transition": null,
  "auth_requirement": "cooperative"
}
```

*Cooperative claims tranche 2, if within the window. Same inclusive deadline boundary as `claim_advance_1`.*

### 13. `reclaim_advance_2`

```json
{
  "name": "reclaim_advance_2",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "NotYetOpened", "AlreadyClaimed", "AlreadyExpired", "ClaimWindowNotPassed"],
  "state_transition": null,
  "auth_requirement": "buyer"
}
```

*Buyer reclaims tranche 2, if the window has passed unclaimed. Same exclusive deadline boundary as `reclaim_advance_1`.*

### 14. `ready_for_delivery`

```json
{
  "name": "ready_for_delivery",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "Advance2Released", "to": "ReadyForDelivery" },
  "auth_requirement": "cooperative"
}
```

*Advance2Released → ReadyForDelivery. The cooperative signaling "setting out for delivery" — opens the remainder-payment window. Auth-gated.*

### 15. `fund_remainder`

```json
{
  "name": "fund_remainder",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "RemainderAlreadyFunded", "RemainderWindowPassed"],
  "state_transition": null,
  "auth_requirement": "buyer"
}
```

*Buyer escrows `total_amount - deposit` — the second half of two-phase funding. Must happen within the window `ready_for_delivery` opened.*

### 16. `expire_remainder_window`

```json
{
  "name": "expire_remainder_window",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "RemainderAlreadyFunded", "RemainderWindowNotPassed"],
  "state_transition": { "from": "ReadyForDelivery", "to": "Defaulted" },
  "auth_requirement": "permissionless"
}
```

*Buyer-default path: `remainder_deadline` has strictly passed with the remainder never funded. Sweeps whatever's currently escrowed to the cooperative and sets `Defaulted`. Permissionless.*

### 17. `reclaim_on_nondelivery`

```json
{
  "name": "reclaim_on_nondelivery",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "DeliveryDeadlineNotPassed"],
  "state_transition": { "from": "Locked|Advance1Released|CheckpointPassed|Advance2Released|ReadyForDelivery", "to": "Forfeited" },
  "auth_requirement": "buyer"
}
```

*Seller-non-delivery path: `delivery_deadline` has strictly passed without `confirm_delivery` ever having run. Returns whatever's currently escrowed to the buyer and sets `Forfeited`. Buyer-gated.*

### 18. `confirm_delivery`

```json
{
  "name": "confirm_delivery",
  "args": [
    { "name": "delivered_quantity", "type": "u32" },
    { "name": "grade_index", "type": "u32" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "RemainderNotFunded", "InvalidGradeIndex"],
  "state_transition": { "from": "ReadyForDelivery", "to": "Delivered" },
  "auth_requirement": "warehouse_operator"
}
```

*ReadyForDelivery → Delivered. Warehouse-operator-attested delivery confirmation. Applies the PRD §7 shortfall/grade adjustment schedule.*

### 19. `settle`

```json
{
  "name": "settle",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "TrancheUnresolved", "FxNotResolved", "FxShortfallUnfunded"],
  "state_transition": { "from": "Delivered", "to": "Settled" },
  "auth_requirement": "permissionless"
}
```

*Delivered → Settled. Splits the contract's remaining token balance between the cooperative and a shortfall refund to the buyer. Requires both advance tranches already resolved (claimed or expired). Oracle-configured commitments additionally require `resolve_fx_shortfall` to have run and any found shortfall to be funded.*

### 20. `resolve_fx_shortfall`

```json
{
  "name": "resolve_fx_shortfall",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "TrancheUnresolved", "FxAlreadyResolved", "OracleNotConfigured"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*PRD §4.2 option (b): reads a fresh oracle rate, reprices `oracle_config.denominated_amount` (adjusted by `settlement_bps`) into the settlement token, and compares that against what's actually escrowed. Computes `fx_shortfall_amount` for `fund_fx_shortfall` to collect. One-time — re-resolving later is not allowed.*

### 21. `fund_fx_shortfall`

```json
{
  "name": "fund_fx_shortfall",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "FxNotResolved", "NoFxShortfall", "FxShortfallAlreadyFunded", "FxShortfallWindowPassed"],
  "state_transition": null,
  "auth_requirement": "buyer"
}
```

*Buyer pays in the shortfall `resolve_fx_shortfall` computed. Buyer-gated, matching `fund_remainder`'s own reasoning. Deadline-inclusive, exactly like `fund_remainder`.*

### 22. `expire_fx_shortfall_window`

```json
{
  "name": "expire_fx_shortfall_window",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "FxNotResolved", "NoFxShortfall", "FxShortfallAlreadyFunded", "FxShortfallWindowNotPassed"],
  "state_transition": { "from": "Delivered", "to": "Defaulted" },
  "auth_requirement": "permissionless"
}
```

*FX-shortfall buyer-default path: `fx_shortfall_deadline` has strictly passed with the top-up never funded. Sweeps whatever's currently escrowed to the cooperative and sets `Status::Defaulted` — the same status `expire_remainder_window`'s buyer-default already uses, by explicit product decision.*

### 23. `cancel`

```json
{
  "name": "cancel",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "Draft|Locked|Advance1Released|CheckpointPassed|Advance2Released|ReadyForDelivery", "to": "Cancelled" },
  "auth_requirement": "buyer_and_cooperative"
}
```

*Mutual cancellation — PRD §7: "Defined unwind: advance settled per agreed schedule, remaining escrow returned, no penalty, logged." Allowed from any pre-delivery state. Requires both buyer's and cooperative's auth in the same call.*

### 24. `reassign_buyer`

```json
{
  "name": "reassign_buyer",
  "args": [
    { "name": "new_buyer", "type": "Address" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": null,
  "auth_requirement": "buyer_cooperative_and_new_buyer"
}
```

*Buyer-position assignability (PRD §4.8): transfers `buyer` to `new_buyer`. Requires three signatures: current buyer, cooperative, and new buyer.*

### 25. `flag_dispute`

```json
{
  "name": "flag_dispute",
  "args": [
    { "name": "flagger", "type": "Address" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "NotAParty"],
  "state_transition": { "from": "Locked|Advance1Released|CheckpointPassed|Advance2Released|ReadyForDelivery|Delivered", "to": "Disputed" },
  "auth_requirement": "flagger"
}
```

*Dispute flagging (PRD's must-have "dispute flagging with defined escalation"). Any one of the three named parties can freeze the commitment by moving it to `Status::Disputed`, which blocks every other state-changing call.*

### 26. `resolve_dispute`

```json
{
  "name": "resolve_dispute",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState"],
  "state_transition": { "from": "Disputed", "to": "<previous_status>" },
  "auth_requirement": "buyer_cooperative_warehouse_operator"
}
```

*Resolves a dispute by unanimous consent — all three named parties' auth. Restores `status` to exactly whatever it was the moment `flag_dispute` ran. Deadline-free on purpose.*

### 27. `expire_dispute_window`

```json
{
  "name": "expire_dispute_window",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "DisputeWindowNotPassed"],
  "state_transition": { "from": "Disputed", "to": "<previous_status>" },
  "auth_requirement": "permissionless"
}
```

*The dispute-side escape hatch: permissionless, deadline-gated. If the three parties can't reach unanimous consent before `dispute_deadline`, this restores the pre-dispute status, exactly like `resolve_dispute` would.*

### 28. `get_status`

```json
{
  "name": "get_status",
  "args": [],
  "returns": "Result<Status, Error>",
  "errors": ["NotInitialized"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — read-only view, callable by anyone.*

### 29. `get_commitment`

```json
{
  "name": "get_commitment",
  "args": [],
  "returns": "Result<Commitment, Error>",
  "errors": ["NotInitialized"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — read-only view, returns the full commitment state.*

---

## Summary

| Total Methods | 29 public methods |
|---|---|
| Auth-gated | 11 methods require explicit auth (buyer/cooperative/warehouse_operator) |
| Permissionless | 18 methods are permissionless reads or state transitions that don't require auth |
| State transitions | 12 methods change contract status, 7 are pure reads |
| Error variants | 38 distinct `Error` variants across all methods |

*This specification is deliberately JSON-structured so that SDK generators, test-harness automata, or a second independent client implementation can programmatically discover and validate every call site against the contract's actual interface, without needing to parse Rust doc comments or guess at argument ordering.*