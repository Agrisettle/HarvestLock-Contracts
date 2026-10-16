# Decision Record: Why Soroban, Not an EVM-Compatible Chain

## Context
This contract (`harvestlock-escrow`) was built on Soroban (Stellar's smart contract platform) rather than deploying to an EVM-compatible chain such as Ethereum or an EVM layer-2. This document explains the specific design requirements of this contract that make Soroban the natural choice, grounded in the contract's actual architecture rather than generic Stellar-vs-Ethereum talking points.

## 1. Claimable-Balance-Style Tranches as Native State

### The Requirement
This contract uses "claimable-balance-equivalent semantics" for its two advance tranches. Each tranche has:
- A claim window (the cooperative can claim within it)
- A reclaim window (the buyer can reclaim after the cooperative's window passes)
- A deadline boundary that strictly separates the two parties' rights

### Why Harder on EVM
On an EVM-compatible chain, there is no native contract-callable primitive equivalent to Stellar's `ClaimableBalanceEntry`. To replicate this behavior on Ethereum, you would need to:

1. **Build ERC-1404-style restricted token transfers** — EVM does have ERC-1404 (restricted transfer), but it's an onerous ERC that requires every recipient to whitelist the contract, adding gas cost and UX friction for every claimant.

2. **Use ERC-721 or ERC-1155 with custom claim logic** — You could model each tranche as a non-fungible position, but this perverts the token semantics and still requires off-chain coordination to manage the claim/reclaim windows.

3. **Rely on `approve`/`transferFrom` with time-lock puzzles** — EVM contracts can simulate deadline-gated access via `block.timestamp` checks, but there's no built-in "this window is now closed" mechanism. You'd need to:
   - Store a `claimed` boolean per tranche
   - Have the cooperative call `claim()` within the window
   - Have the buyer call `reclaim()` after the window passes
   - But there's no enforcement that only one of these paths succeeds — you'd need additional logic to prevent double-claiming, and the `approve`/`transferFrom` pattern opens reentrancy risk that Soroban avoids with its Checks-Effects-Interactions ordering and native auth model.

4. **Classic Stellar `ClaimableBalanceEntry` interop is unavailable** — The contract explicitly reimplements claimable-balance semantics natively (stored deadline + two booleans: `claimed` + `expired`) because Soroban contracts **cannot** construct a classic-ledger `ClaimableBalanceEntry` from within contract code without cross-VM interop. On EVM, there's no equivalent "native claimable balance" primitive to interop with — you'd be building from scratch, and the natural EVM approach (ERC-20 + timelock) doesn't give you the same single-contract audit surface.

### Why Easier on Soroban
Soroban provides the primitives to build this natively:

- **Stored deadline + two booleans** (`advance1_deadline`, `advance1_claimed`, `advance1_expired` and similarly for tranche 2) — one implementation, four public wrappers (`claim_advance_1`, `claim_advance_2`, `reclaim_advance_1`, `reclaim_advance_2`), and the rules can't drift between tranches by construction (see `Tranche` enum parameterizing `open_tranche`/`claim_tranche`/`reclaim_tranche`).

- **Checks-Effects-Interactions ordering** is enforced by the Soroban VM: state is mutated and saved *before* the external token transfer in every function (`lock`, `claim_tranche`, `reclaim_tranche`, `settle`). This costs nothing in the failure case (a panic unwinds the entire invocation including earlier storage writes) and only helps in a reentrancy scenario. On EVM, you'd have to consciously adopt this pattern; here it's the default.

- **One contract instance per commitment** (PRD §4.8) means there's no need for a commitment ID parameter — the contract *is* the commitment. On EVM, you'd typically need a registry pattern mapping commitment IDs to contract instances, adding an extra layer of indirection and state.

## 2. Native Multi-Party Authorization

### The Requirement
This contract supports multi-party operations where three named parties (buyer, cooperative, warehouse_operator) must collectively authorize state transitions. Key examples:
- `reassign_buyer`: three-party consent to transfer the buyer position
- `resolve_dispute`: unanimous three-party consent to resolve a dispute
- `flag_dispute`: any one of the three named parties can flag a dispute
- `set_allocation`: cooperative-gated allocation ledger capture

### Why Harder on EVM
EVM's authorization model is fundamentally different:

1. **EVM has no concept of per-entry Soroban `SorobanAuthorizationEntry`** — EVM uses simple ECDSA signature recovery (v, r, s) from the transaction's single signing account. There's no "authorizeEntry" equivalent that binds a signature to a specific sub-invocation within a contract call.

2. **`require_auth()` per named party doesn't compose** — The HANDOFF.md explicitly documents a latent bug: Soroban's auth manager rejects a *second* `require_auth()` for the **same** address within a single call frame (`Auth/InvalidAction`). On EVM, you could simulate this with multi-sig wallets (Gnosis Safe, etc.), but:
   - Multi-sig introduces a trusted third party
   - Each party's approval transaction is separate, not atomic with the contract call
   - There's no way to say "this transaction is authorized by parties A, B, and C collectively for this specific operation" without wrapping everything in a multi-sig deposit/withdraw pattern

3. **`require_once_each` pattern** — This contract uses `require_once_each` in `cancel`, `reassign_buyer`, and `resolve_dispute`, which calls `require_auth()` once per *distinct* address. This was a hard-won design decision because `mock_all_auths()` (the test auth mode) tolerates duplicate `require_auth()` calls, but real Soroban auth does not. An EVM equivalent would need explicit "no duplicate signer" logic in the contract, which doesn't exist natively.

4. **Dispute freeze/resume with three parties** — `flag_dispute` lets any one of three parties freeze a commitment into `Disputed`. `resolve_dispute` requires unanimous three-party consent. `expire_dispute_window` is permissionless once the deadline passes. Replicating this on EVM would require:
   - A custom event-based signaling system
   - Off-chain coordination to manage the freeze/resume lifecycle
   - No native "this commitment is frozen" primitive that integrates with EVM's transfer model

### Why Easier on Soroban
Soroban's auth model is built for exactly this kind of multi-party logic:

- **`require_once_each`** — Calls `require_auth()` once per distinct address, preventing the `Auth/InvalidAction` error that would abort the entire invocation. This is a first-class language/runtime feature, not a library pattern.

- **Per-entry SorobanAuthorizationEntry** — Each non-source party's signature is bound to the specific invocation tree (contract + fn_name + args + sub-invocations). This means you can have a single transaction that's authorized by multiple parties for different parts of the operation, something EVM cannot do without multi-sig wrappers.

- **`mock_auths()` vs `mock_all_auths()`** — The test infrastructure itself reflects the auth model's maturity: `mock_all_auths()` records all auth entries and enforces the no-duplicate rule, while `mock_auths()` (older mode) tolerates duplicates for backward compatibility. This dual-mode testing would be equivalent to having both "strict multi-sig" and "simple ECDSA sign" modes on EVM, but Soroban has this built into the runtime.

## 3. Fiat-Rate Oracle Integration

### The Requirement
This contract integrates with a Reflector SEP-40 oracle for fiat currency conversion (PRD §4.2 option (b), §16.3 staleness bound). The oracle config is `Option<OracleConfig>` — `None` for a plain deal, `Some` for a deal denominated in a currency the settlement token doesn't natively track (e.g., NGN). The contract:
- Calls `oracle_rate()` to read a fresh rate
- Enforces a caller-configured `max_age_secs` staleness bound
- Uses the rate in `resolve_fx_shortfall` to compute `fx_adjusted_total` and `fx_shortfall_amount`
- Wires option (b) into `settle`'s payout math: the cooperative receives the fresh-rate-converted amount, not the stale `total_amount`

### Why Harder on EVM
EVM oracle integration faces fundamentally different constraints:

1. **No built-in SEP-40 oracle interface** — EVM contracts typically rely on:
   - Decentralized price feeds (Chainlink, Band, DIA)
   - Contract-specific oracle designs (AggLayer, futarchy markets)
   - Each of these has a different ABI, and none maps 1:1 to the Reflector SEP-40 `lastprice`/`price`/`timestamp` shape this contract uses.

2. **Staleness bound enforcement** — PRD §16.3 requires that a quote older than `max_age_secs` be rejected. On EVM, you'd need to:
   - Store the quote timestamp on-chain
   - Compare `block.timestamp` against it in every oracle-read function
   - But `block.timestamp` on EVM is often manipulated by miners (though less so than commonly claimed, it's still not as rigorously bounded as Soroban's ledger timestamp). The Soroban VM guarantees that `ledger().timestamp()` is the immutable ledger header timestamp, and the VM will panic if a contract tries to read a different source.

3. **Cross-contract call cost and semantics** — On Soroban, `ReflectorPulseClient` generates a cross-contract call that matches on XDR wire shape, not Rust type identity. This means an independently-defined `Asset`/`PriceData` pair with the same shape as `reflector.rs` is indistinguishable from the genuine Reflector contract, from `oracle_rate`'s perspective. On EVM, each oracle integration requires a custom ABI decode, and there's no "match on wire shape" abstraction — you're either using a library (e.g., Chainlink AggregatorV2) or rolling your own contract interface.

4. **FX shortfall computation in settlement** — The contract's `settle` pays out against `settlement_bps` (computed at `confirm_delivery` time), and `resolve_fx_shortfall` compares the fresh-rate-converted `denominated_amount` against what's actually escrowed. This two-step process (confirm_delivery → resolve_fx_shortfall → settle) is unique to this contract's design, and wiring it into an EVM settlement contract would require:
   - Separate "oracle read" and "shortfall resolution" transactions
   - Careful ordering to ensure the FX rate can't change between steps (reentrancy risk)
   - On EVM, you'd typically need a keeper pattern or commit-reveal to prevent front-running the FX rate

### Why Easier on Soroban
Soroban's oracle integration is streamlined:

- **`ReflectorPulseClient`** — A generated client trait that cross-calls the Reflector contract on XDR wire shape. The contract doesn't need to depend on a Rust crate; it only needs the trait definition (in `reflector.rs`), and the cross-contract call matches on XDR shape, not Rust type identity.

- **`Option<OracleConfig>`** — The `initialize` function's oracle config is genuinely optional (`None` works fine). This means plain deals (no FX risk) don't pay any oracle overhead, and FX-configured deals just pay the extra storage cost for the `OracleConfig` key.

- **Staleness bound as a first-class error** — `OracleStale` is a concrete `Error` variant, not a silently-old number that silently changes the payout. The Soroban VM enforces the `max_age_secs` check every time, and the contract can't accidentally return a too-old rate.

- **One-time computation** — `resolve_fx_shortfall` is one-time and immutable (same reasoning as `set_allocation`). Re-resolving later against a different rate would let either side game the number by choosing when to call it — the contract prevents this by design.

## 4. Two-Phase Funding and Buyer Default as Deadline

### The Requirement
This contract uses two-phase funding:
- `lock` escrows only the deposit (`advance1_bps + advance2_bps` of `total_amount`)
- `fund_remainder` escrows the remainder (`total_amount - deposit`) after `ready_for_delivery` opens the window
- `expire_remainder_window` (permissionless) sweeps the remaining balance to the cooperative if the buyer doesn't fund before the deadline

This design makes "buyer default" a contract-enforceable deadline rather than something requiring a person to assert fault.

### Why Harder on EVM
On EVM, two-phase funding patterns typically look like:

1. **Full escrow at deposit time** — The buyer deposits the full `total_amount` at contract creation, and the contract releases it according to a schedule. This is the simplest EVM pattern (e.g., a simple escrow contract with `release_after(deadline)`). But it makes "buyer default" harder: if the buyer already deposited everything, there's no on-chain signal that they *failed* to do something — the money is already there. You'd need an off-chain monitor to detect the deadline and trigger a cleanup transaction.

2. **Split escrow with timelock** — Some EVM contracts split the deposit into tranches with timelocks, but:
   - There's no native "this window is now closed" signal that's unauthenticated
   - You'd need a guardian pattern (off-chain watcher + multi-sig) to trigger the default, reintroducing the human-assertion problem this contract's design specifically avoids

3. **No permissionless expiry** — EVM contracts can enforce deadlines via `require` checks on `now > deadline`, but there's no equivalent to Soroban's `expire_remainder_window` being permissionless. On EVM, someone always has to pay gas to trigger the expiry — typically the beneficiary (cooperative) or a guardian. Making it permissionless (anyone can call) is possible but unusual in EVM smart contract patterns; usually the "interested party" triggers the state change.

### Why Easier on Soroban
Soroban's design makes the permissionless expiry natural:

- **`expire_remainder_window` is permissionless** — The outcome (sweep to cooperative) doesn't depend on who calls it, only on whether the deadline has passed. This is explicitly noted in the HANDOFF.md and the module doc: "Anyone — including an off-chain watcher — can trigger it once the fact of the matter (deadline passed, unfunded) is true."

- **Same deadline pattern as tranche resolution** — The four deadline-boundary tests (#112–#115) all use the same pattern: the deadline timestamp is the *last* second on which the original window is still open, and every deadline-triggered failure path opens strictly after it. This consistency means the contract's auth model, event surface, and deadline semantics all reuse the same underlying patterns.

- **`Defaulted` vs `Forfeited` are distinct** — The contract deliberately keeps these two status variants separate (see the `Status` enum doc comment): `Defaulted` = buyer failed to fund remainder; `Forfeited` = cooperative never delivered. Collapsing them into one status would make an already-settled commitment's history ambiguous about who failed. On EVM, you'd typically have one "failed" status and infer fault from *which function* fired, which this contract's design explicitly avoids.

## 5. Allocation Ledger: Record-Only in v1

### The Requirement
`set_allocation` records each member farmer's entitlement share as a salted hash (per-member HMAC-SHA256(salt, phone_number) computed off-chain). `settle` pays the cooperative a lump sum; the ledger provides on-chain transparency into what each member is owed off-chain. This is PRD §4.9 Rung 1 (Transparency Ladder Rung 1 — "Payment settles to the cooperative wallet. Each member's entitlement is on chain and readable; members receive an SMS stating their share") — not a deferred design decision.

### Why Harder on EVM
1. **Per-member salted hashes** — On EVM, you'd need to:
   - Store `bytes32 member_hash` + `uint16 share_bps` per member
   - Compute the HMAC off-chain (same as Soroban), but storing and updating per-member entries in a dynamic `Mapping` or `Array` is gas-intensive
   - The `MAX_ALLOCATION_MEMBERS = 200` cap (anchored to PRD §16.3) is generous, but on EVM, each additional member adds `~200-400 gas` for storage writes, plus the overhead of keccak256 computation if done on-chain

2. **NDPA erasability** — The salt+phone-number mapping lives only in the API's Postgres. Deleting the off-chain row makes the on-chain hash permanently unlinkable to a real person, since brute-forcing a random salt is infeasible. On EVM, there's no analogous "off-chain mapping" pattern built into the VM — you'd need to design the erasability mechanism from scratch, and the on-chain hash would need to be explicitly designed with erasability in mind from day one.

3. **Settlement is still a lump sum** — Even with the allocation ledger on-chain, `settle` pays the cooperative a single transfer. On EVM, pro-rated payout across N members would require N token transfers in a single transaction, which hits gas limits quickly and requires ERC-865 or ERC-20 top-up patterns for recipients who don't hold the settlement token natively.

### Why Easier on Soroban
- **Per-member salted hashes are native** — `AllocationMember { member_hash: BytesN<32>, share_bps: u32 }` is a first-class contract type. The salt is computed off-chain (API), and the on-chain entry is just the hash + share — no keccak256 cost inside the contract.

- **Record-only, lump-sum settle** — Matches the PRD's v1 default explicitly. The contract doesn't need to pro-rate, which keeps `settle` gas-cost O(1) regardless of membership size.

- **One-time, immutable** — `set_allocation` can only be called once (`AllocationAlreadySet` guard). No amend function exists, which simplifies the contract state and eliminates a whole class of update bugs.

## Conclusion

This contract's specific design requirements — claimable-balance-style tranches built natively, multi-party authorization with `require_once_each`, a Reflector SEP-40 oracle with staleness bounds, permissionless deadline-gated expiry, and a salted-hash allocation ledger — map most naturally to Soroban's runtime primitives:

| Design Feature | Soroban Primit | EVM Equivalent | Relative Difficulty |
|---|---|---|---|
| Claimable-balance-style tranches | Stored deadline + two booleans; native `claim_tranche`/`reclaim_tranche` | ERC-20 + timelock + custom claimed/expired booleans; no native claimable balance | **Much easier on Soroban** |
| Multi-party auth (`require_once_each`) | First-class runtime feature; per-entry `SorobanAuthorizationEntry` | Multi-sig wrappers; no per-entry auth; duplicate `require_auth()` aborts the whole call | **Much easier on Soroban** |
| Reflector SEP-40 oracle | `ReflectorPulseClient`; XDR-shape matching; `Option<OracleConfig>` | Custom oracle ABIs (Chainlink, etc.); no XDR-shape abstraction; each integration is bespoke | **Easier on Soroban** |
| Permissionless deadline expiry | `expire_remainder_window` / `expire_fx_shortfall_window` — anyone can call | Requires guardian/multi-sig pattern; no native permissionless expiry primitive | **Easier on Soroban** |
| Salted-hash allocation ledger | `AllocationMember { member_hash: BytesN<32>, share_bps: u32 }` — one-time, immutable | Per-member `Mapping` + `keccak256` gas cost; no built-in erasability pattern; pro-rated settle requires N transfers | **Easier on Soroban** |

Generic Stellar-vs-Ethereum talking points (e.g., "lower fees", "different consensus") are irrelevant compared to the concrete architecture-driven reasons above. This contract was built on Soroban because its runtime features directly support the design patterns this contract needs — and those same patterns would require substantial additional contracts, off-chain coordination, or semantic compromises on EVM.

---
*This decision record is grounded in the contract's actual design as documented in HANDOFF.md and lib.rs module comments, the live testnet deployments described in CHANGELOG.md, and the 120+ unit tests in test.rs that validated the behavior across all state transitions and error paths.*