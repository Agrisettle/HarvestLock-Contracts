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
    { "name": "terms", "type": "CommitmentTerms" },
    { "name": "oracle_config", "type": "Option<OracleConfig>" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["AlreadyInitialized", "ZeroAmount", "InvalidBps", "InvalidWindow", "InvalidQuantity", "InvalidGradeSchedule", "GradeScheduleTooLong", "InvalidOracleConfig"],
  "state_transition": { "from": "Draft", "to": "Locked" },
  "auth_requirement": "buyer"
}
```

*Creates the commitment in `Draft`. No funds move yet. Requires the buyer's auth.*

`CommitmentTerms` is the one agreed-terms struct (#270), shared with
`get_commitment_view`'s `terms` field so generated clients, indexers and this
document all use the same field names:

| field | type | meaning |
|---|---|---|
| `total_amount` | `i128` | the deal's total value; `> 0` and `<= MAX_TOTAL_AMOUNT` |
| `advance1_bps` | `u32` | basis points of `total_amount` released at lock-in |
| `advance2_bps` | `u32` | basis points released at the mid-season checkpoint; `advance1_bps + advance2_bps <= 10_000` |
| `claim_window_secs` | `u64` | how long the cooperative has to claim an opened advance; `> 0` |
| `remainder_window_secs` | `u64` | how long the buyer has to fund the remainder (also the FX cure window and the dispute-freeze bound); `> 0` |
| `delivery_window_secs` | `u64` | how long after creation the cooperative has to reach `Delivered`; `> 0` |
| `contracted_quantity` | `u32` | contracted quantity in caller-defined units; `> 0` |
| `grade_price_bps` | `Vec<u32>` | grade -> price multiplier schedule; non-empty, at most `MAX_GRADE_ENTRIES` entries, every entry `<= 10_000` |

Before #270 these eight terms were positional arguments. Three adjacent
`u64` windows and two adjacent `u32` bps values meant a swap type-checked and
silently produced a different deal; named fields make that impossible, and
the generated bindings carry the names through. `initialize`'s `terms` is the
same type `get_commitment_view` returns, so a client can round-trip the
values it reads without renaming a single field.

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

### 3. `replace_oracle_config`

```json
{
  "name": "replace_oracle_config",
  "args": [
    { "name": "new_config", "type": "OracleConfig" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "FxAlreadyResolved", "OracleNotConfigured", "InvalidOracleConfig"],
  "state_transition": null,
  "auth_requirement": "buyer_and_cooperative"
}
```

*Replaces a broken oracle configuration (#215), e.g. an unreachable oracle contract. Only legal in `Delivered` while `fx_resolved` is false, and only on a commitment that already has an oracle configured. The new config is validated like `initialize`'s (`max_age_secs != 0`). Requires both the buyer's and the cooperative's auth. Emits `OracleConfigReplaced`.*

### 4. `oracle_rate`

```json
{
  "name": "oracle_rate",
  "args": [],
  "returns": "Result<OracleRate, Error>",
  "errors": ["OracleNotConfigured", "OraclePriceUnavailable", "OracleStale", "OracleTimestampInFuture"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*Reads the current rate for this commitment's configured oracle asset, enforcing the staleness bound and rejecting quote timestamps later than the ledger timestamp. Pure read against Reflector's own stored history, not a state transition.*

### 5. `set_allocation`

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

### 6. `get_allocation`

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

### 7. `extend_delivery_deadline`

```json
{
  "name": "extend_delivery_deadline",
  "args": [
    { "name": "new_deadline", "type": "u64" }
  ],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "DeliveryDeadlinePassed", "DeliveryDeadlineNotExtended"],
  "state_transition": null,
  "auth_requirement": "buyer_and_cooperative"
}
```

*Moves `delivery_deadline` later by mutual consent while the deal is pre-delivery (`Locked` through `ReadyForDelivery`) and the current deadline has not passed. Monotonic: `new_deadline` must be strictly later than the current deadline, so time already promised to the cooperative can never be taken back. Requires both the buyer's and the cooperative's auth.*

### 8. `lock`

```json
{
  "name": "lock",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "LockAfterDeliveryDeadline"],
  "state_transition": { "from": "Draft", "to": "Locked" },
  "auth_requirement": "buyer"
}
```

*Draft → Locked. Pulls the buyer's deposit into the contract — `advance1_bps + advance2_bps` of `total_amount`, not the full amount. No prior SEP-41 `approve()` needed.*

*Refused once `delivery_deadline` has strictly passed (#277): the delivery window counts from `initialize`, not from `lock`, so a late lock would otherwise hand the buyer an immediate `reclaim_on_nondelivery`. The deadline second itself is still in time.*

### 9. `release_advance_1`

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

### 10. `claim_advance_1`

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

### 11. `reclaim_advance_1`

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

### 12. `mark_checkpoint`

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

### 13. `release_advance_2`

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

### 14. `claim_advance_2`

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

### 15. `reclaim_advance_2`

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

### 16. `ready_for_delivery`

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

*Advance2Released → ReadyForDelivery. The cooperative signaling "setting out for delivery" — opens the remainder-payment window. Auth-gated. When the advance split sums to exactly 10 000 bps the deposit already covers `total_amount`, so the remainder is zero and this call marks it funded itself (#275), instead of requiring a `fund_remainder` call that would transfer nothing.*

### 17. `fund_remainder`

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

*Buyer escrows `total_amount - deposit` — the second half of two-phase funding. Must happen within the window `ready_for_delivery` opened. A zero remainder (advance bps summing to 10 000) is already funded by `ready_for_delivery`, so this refuses with `RemainderAlreadyFunded` rather than transferring zero tokens (#275).*

### 18. `expire_remainder_window`

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

### 19. `reclaim_on_nondelivery`

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

### 20. `expire_delivery_window`

```json
{
  "name": "expire_delivery_window",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "DeliveryGraceWindowNotPassed"],
  "state_transition": { "from": "Locked|Advance1Released|CheckpointPassed|Advance2Released|ReadyForDelivery", "to": "Forfeited" },
  "auth_requirement": "permissionless"
}
```

*Permissionless fallback for `reclaim_on_nondelivery` when the buyer's key is lost: once `delivery_deadline + remainder_window_secs` (the grace period) has strictly passed, anyone can return whatever's escrowed to the buyer and set `Forfeited`. Without it, a lost buyer key would lock the funds forever.*

### 21. `confirm_delivery`

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

### 22. `settle`

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

### 23. `resolve_fx_shortfall`

```json
{
  "name": "resolve_fx_shortfall",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "TrancheUnresolved", "FxAlreadyResolved", "OracleNotConfigured", "OracleTimestampInFuture"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*PRD §4.2 option (b): reads a fresh oracle rate, reprices `oracle_config.denominated_amount` (adjusted by `settlement_bps`) into the settlement token, and compares that against what's actually escrowed. Computes `fx_shortfall_amount` for `fund_fx_shortfall` to collect. One-time — re-resolving later is not allowed.*

### 24. `refresh_fx_quote_status`

```json
{
  "name": "refresh_fx_quote_status",
  "args": [],
  "returns": "Result<(), Error>",
  "errors": ["InvalidState", "FxAlreadyResolved", "OracleNotConfigured", "OracleUnavailable", "OraclePriceUnavailable", "OraclePriceInvalid", "OracleTimestampInFuture"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*Stale-quote probe (#1). Runs the same oracle read `resolve_fx_shortfall` would and records the result in `Commitment::fx_quote_stale`: `true` when the quote is too old (`OracleStale` is recorded, not returned), `false` when it is fresh. Emits `FxQuoteStaleChanged` only when the flag changes. Other oracle failures are returned as errors so callers can see why settlement is stuck.*

*Informational only: no guard reads the flag, and `resolve_fx_shortfall` still checks staleness itself. Only legal in `Delivered` on an oracle-configured commitment whose FX step hasn't been resolved yet. Permissionless: it moves no funds, grants nothing and takes no outcome parameter.*

### 25. `fund_fx_shortfall`

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

### 26. `expire_fx_shortfall_window`

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

### 27. `cancel`

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

### 28. `reassign_buyer`

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

### 29. `flag_dispute`

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

### 30. `resolve_dispute`

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

### 31. `expire_dispute_window`

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

### 32. `get_status`

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

### 33. `get_commitment`

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

### 34. `get_commitment_view`

```json
{
  "name": "get_commitment_view",
  "args": [],
  "returns": "Result<CommitmentView, Error>",
  "errors": ["NotInitialized"],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — the versioned read model (#256): `schema_version`, `status`, `created_at`, and the `parties` / `terms` / `tranches` / `funding` / `delivery` / `fx` / `dispute` sub-structs. Storage-layout changes to `Commitment` do not change this shape; only a `schema_version` bump does. `fx.configured` distinguishes a commitment with no oracle from one resolved with zero values.*

---

### 35. `available_actions`

```json
{
  "name": "available_actions",
  "args": [],
  "returns": "Vec<Action>",
  "errors": [],
  "state_transition": null,
  "auth_requirement": "permissionless"
}
```

*No auth required — read-only view, callable by anyone. Returns every state-changing call the current commitment currently permits, each as an `Action { function, auth, deadline }` where `function` is the Soroban symbol to invoke, `auth` is the party whose `require_auth()` the call needs (or `Anyone` for permissionless calls), and `deadline` bounds the call by timestamp (`None` when no timing gate applies, `Inclusive(T)` when legal up to and including `T`, `Exclusive(T)` when legal only strictly after `T`).*

*Every entry is produced by the same private guard predicate (`can_lock`, `can_claim_tranche`, `can_settle`, …) the real method calls, so the view cannot drift from the checks that actually authorize the call. `initialize` is deliberately excluded (there is no commitment to derive actions from before it runs — an empty list is returned), and the read-only getters/`oracle_rate` are excluded (they do not change state and are always callable). Oracle-backed actions are advertised purely from local state (a configured oracle and an unresolved FX step); quote freshness is an external read, so an advertised `resolve_fx_shortfall` can still return `Error::OracleStale` until the oracle publishes a fresh quote — the `fx_quote_stale` flag on `get_commitment` is the companion "waiting on a fresh quote" signal.*

---

## Summary

| Total Methods | 35 public methods |
|---|---|
| Auth-gated | 19 methods require explicit auth (buyer/cooperative/warehouse_operator/new buyer/flagger) |
| Permissionless | 16 methods are permissionless reads or state transitions that don't require auth |
| State transitions | 17 methods declare a `state_transition`, 7 are read-only views |
| Error variants | 45 distinct `Error` variants across all methods |

*`.github/scripts/check_interface_spec.sh` (run in CI) fails if a `pub fn` in the `#[contractimpl]` block has no section here, a section names a method that doesn't exist, or the counts above stop matching the JSON blocks.*

*This specification is deliberately JSON-structured so that SDK generators, test-harness automata, or a second independent client implementation can programmatically discover and validate every call site against the contract's actual interface, without needing to parse Rust doc comments or guess at argument ordering.*