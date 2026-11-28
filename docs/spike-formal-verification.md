# Spike: Formal Verification Feasibility

**Issue Reference:** Closes #48 | Fixes https://github.com/Agrisettle/HarvestLock-Contracts/issues/48  
**Scope:** Formal verification tooling ecosystem for Soroban / Rust smart contracts and applicability to HarvestLock.

---

## 1. Executive Summary

This spike investigates the current landscape of formal verification (FV) tooling for Soroban/Rust smart contracts and assesses whether `HarvestLock-Contracts` is a viable candidate for FV prior to external security audit.

**Core Findings:**
- **Tooling Landscape:** The formal verification ecosystem for Soroban smart contracts is nascent. Unlike EVM (which has mature FV ecosystems including Certora Prover, K-framework/KEVM, Halmos, and Act) and Solana (which has early SeaHorn/Kani ports), Soroban does not yet have an off-the-shelf, dedicated formal prover tailored for Soroban SDK host environments and storage abstractions.
- **Applicable Rust FV Tools:** General-purpose Rust verification tools exist (notably **Kani** from Amazon and **Prusti** for verification of Rust code with Viper), but they lack native mock models for Soroban host environment functions (`soroban_sdk::Env`, host storage maps, crypto builtins, and cross-contract calls).
- **Contract Characteristics:** `HarvestLock` is compact (~2,500 lines of Rust logic in `lib.rs`), with a deterministic finite-state machine (Initialized -> Locked -> RemainderFunded -> Settled/Cancelled/Disputed). The transitions, deadline checks, and arithmetic invariants (e.g. `deposit_bps + advance1_bps + advance2_bps <= 10_000`) are well-specified.
- **Current State of the Art for Soroban:** Model checking and symbolic execution at the Wasm level (e.g., via Wasm-specialized tools) or property-based fuzzing (via `proptest` / `cargo-fuzz`) provide the highest practical ROI today on Soroban.

**Recommendation:**
Full formal verification using a formal prover is currently infeasible without high-effort custom modeling of the Soroban host environment. Instead, property-based testing and exhaustive state transition fuzzing (such as `proptest`) should serve as the mathematical verification bridge ahead of audit.

---

## 2. Survey of Formal Verification Tooling for Soroban / Rust

| Tool | Ecosystem / Layer | Applicability to Soroban | Feasibility / Readiness |
| :--- | :--- | :--- | :--- |
| **Kani Rust Verifier** (AWS) | Rust MIR / CBMC bounded model checking | High potential for pure algorithmic components; struggles with Soroban host functions (`soroban_sdk::Env`). | **Partial / Experimental**: Can verify arithmetic functions (e.g. `bps_amount`, `checked_fx_conversion`) in isolation, but fails on full contract state machine without comprehensive stubs for Soroban host calls. |
| **Prusti / Creusot** | Rust source / deductive verification | Requires proof annotations in Rust; lacks support for foreign host function calls and Soroban macros. | **Infeasible**: High annotation burden; incompatible with `soroban-sdk` attribute macros and host bindings. |
| **Certora Prover** | EVM bytecode / CVL | EVM only. No Soroban/Wasm support currently available. | **N/A** |
| **Halmos / Kontrol** | EVM symbolic execution | EVM only. | **N/A** |
| **Manticore / Mythril** | EVM / Wasm symbolic execution | Manticore has experimental Wasm support, but lacks Soroban host runtime semantics (storage keys, TTL, auth manager). | **Impractical**: High false positive rate and missing Soroban host hooks. |
| **Proptest / Arbitrary** | Rust property-based testing | Fully supported in Rust and integrates seamlessly with Soroban's unit-testing framework (`soroban-sdk::Env::default()`). | **Ready & Recommended**: Already proven viable for state machine invariants and window overflow testing. |

---

## 3. Candidate Invariants for Formal Proof in HarvestLock

If Soroban host stubs or bounded model checking tools are applied, the following invariants are high-priority candidates:

1. **Balance Conservation Invariant:**
   $\sum \text{escrowed deposits} - \sum \text{payouts} = \text{contract token balance}$. Stroops cannot be created or leaked.
2. **Terminal State Exclusivity:**
   Once in a terminal state (`settled`, `cancelled`, or `disputed_and_resolved`), no subsequent withdrawal or state mutation can occur.
3. **Tranche Mutual Exclusivity:**
   For any tranche $i \in \{1, 2\}$, $claimed_i$ and $reclaimed_i$ cannot both be true simultaneously.
4. **Deadline Monotonicity:**
   Claims are only valid at or before the tranche deadline ($now \le deadline$), while reclaims are only valid strictly after ($now > deadline$).
5. **No Arithmetic Overflow:**
   All basis points arithmetic, token scaling, and FX shortfall calculations are checked and cannot trap or overflow unexpectedly.

---

## 4. Conclusion & Actionable Recommendation

1. **Conclusion:** Dedicated formal verification tooling for Soroban is not yet production-ready for automated audit readiness. Attempting full-contract formal verification today would require authoring and maintaining an ad-hoc formal model of the Soroban host environment.
2. **Actionable Roadmap:**
   - **Pre-Audit Phase (Current):** Expand property-based test suites using `proptest` for all mathematical functions (`bps_amount`, `checked_fx_conversion`) and state transition invariants.
   - **Audit Phase:** Highlight the state transition matrix and invariants to human auditors.
   - **Post-Audit / Future:** Monitor the development of Soroban formal verification initiatives by the Stellar Development Foundation (SDF) and community partners.
