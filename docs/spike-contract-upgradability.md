# Spike: Contract Upgradability

**Issue Reference:** Closes #47 | Fixes https://github.com/Agrisettle/HarvestLock-Contracts/issues/47  
**Scope:** Soroban `upgrade_current_contract_wasm` host function, instance-per-commitment model vs. upgradable contract trade-offs.

---

## 1. Executive Summary

This spike analyzes the feasibility and trade-offs of integrating Soroban's `upgrade_current_contract_wasm` host function into the `HarvestLock` escrow contract.

**Key Architecture Context:**
Under PRD §4.8, `HarvestLock` operates under a **one contract instance per commitment** architecture. Every commitment is a distinct contract deployed on-chain for a single agricultural forward-sale agreement with fixed terms, fixed participants (buyer, cooperative, warehouse operator, arbiter), and finite lifecycles (ranging from weeks to months).

**Recommendation:**
**Retain the deliberately immutable, non-upgradable design.** Do not add `upgrade_current_contract_wasm` to the contract.
- In an instance-per-commitment model, upgradability provides little operational benefit while introducing severe trust, governance, and security risks.
- For bug fixes or logic improvements, new commitments can simply deploy the updated Wasm hash, leaving existing locked commitments to execute under the exact immutable terms agreed upon by the participants at lock-in.

---

## 2. Soroban Upgradability Mechanism (`upgrade_current_contract_wasm`)

Soroban provides native code upgradability via the host environment:
```rust
env.deployer().update_current_contract_wasm(&new_wasm_hash);
```
- **How it works:** When invoked, this host function updates the contract executable code linked to the current contract ID to point to a previously uploaded Wasm hash (`new_wasm_hash`).
- **Data Preservation:** The contract's storage (`instance`, `persistent`, and `temporary`) remains untouched.
- **Requirements:** Any contract exposing this mechanism must protect it with strict authorization guards (e.g., admin signature or multi-sig governance).

---

## 3. Trade-Off Analysis for HarvestLock

| Dimension | Immutability (Current Architecture) | Upgradability (`update_current_contract_wasm`) |
| :--- | :--- | :--- |
| **Commitment Trust Model** | **Zero Governance Risk:** Parties (buyer, cooperative, lender) lock funds knowing the bytecode and transition logic cannot be altered unilaterally. | **High Trust Requirement:** Parties must trust whoever holds upgrade keys not to modify payout rules or siphon escrowed funds. |
| **Instance Lifecycle** | Each instance is transient (created at deal agreement, finalized upon harvest/delivery/dispute settlement). | Instances are still short-lived, meaning code updates would rarely apply to existing commitments before they expire. |
| **Governance Complexity** | None. No admin keys, multi-sig upgrades, or timelocks required in contract state. | Requires introducing an admin role or requiring 3-party unanimous consent (buyer + cooperative + arbiter) to approve any code update. |
| **Attack Surface** | Minimal. Contract logic is fixed and verified at deployment. | High. A compromise of upgrade authority compromises all locked funds; potential storage layout incompatibilities between versions. |
| **Bytecode / Gas Cost** | Lower bytecode size; zero gas overhead for upgrade guards. | Increased Wasm size, storage footprint for admin state, and execution overhead. |
| **Fixing Bugs / Improvements** | Deploy subsequent commitments with the fixed Wasm hash. Legacy contracts execute or trigger clean fallback (`cancel` / `resolve_dispute`). | Can patch a live instance in-flight, but requires coordinated governance authorization. |

---

## 4. Evaluation of Multi-Party Consent Upgrade Alternative

If upgradability were demanded for long-running commitments, the only trust-minimized implementation would require unanimous multi-party consent:
```rust
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
    let c = Self::load(&env)?;
    // Require unanimous authorization from buyer, cooperative, and arbiter
    c.buyer.require_auth();
    c.cooperative.require_auth();
    c.arbiter.require_auth();
    env.deployer().update_current_contract_wasm(&new_wasm_hash);
    Ok(())
}
```
**Why this is still inferior to immutability:**
1. If the parties are already in agreement or in dispute, existing escape hatches (`cancel` with multi-party consent or `resolve_dispute` via arbiter) already exist to unwind or redirect funds safely.
2. In agricultural forward commitments, if trust breaks down, obtaining three valid signatures on a new Wasm hash is virtually impossible.
3. If an automated vulnerability or edge case occurs, coordinating offline verification and signature collection for a new Wasm binary across all non-technical participants (e.g. rural cooperatives) is slower than utilizing existing dispute mechanisms.

---

## 5. Conclusion & Actionable Recommendation

1. **Conclusion:** Adding `upgrade_current_contract_wasm` breaks the core value proposition of HarvestLock escrow — that "every stroop leaves the escrow only where the terms fixed at lock-in say it should go."
2. **Actionable Recommendation:**
   - Keep `HarvestLock-Contracts` deliberately immutable once deployed.
   - For contract evolution, rely on versioned deployments: upgrade the factory/client tooling to deploy new commitment contracts pointing to new Wasm hashes.
   - For catastrophic flaws in locked instances, rely on the established multi-party `cancel` and `resolve_dispute` resolution paths.
