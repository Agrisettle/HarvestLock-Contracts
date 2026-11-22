# Audit Readiness Checklist

This document tracks the per-item status of audit-preparation work for the
HarvestLock escrow contract. Items are grouped into the five categories an
auditor typically reviews before beginning a formal engagement. Each item is
marked **Done**, **Partial**, or **Not done**, with a note explaining the
current state and (where applicable) what remains.

---

## 1. Invariants Documentation

Formal statements of what must always be true inside a contract, independent
of which call path reached the current state.

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1.1 | State-machine invariant documented | **Done** | The module-level doc comment in `lib.rs` defines the full status graph (`Draft → Locked → … → Settled`), terminal statuses (`Cancelled`, `Defaulted`, `Forfeited`, `Disputed`), and the transitions that reach each. |
| 1.2 | Two-phase funding invariant documented | **Done** | Module doc explains that `lock` escrows only the deposit (`advance1_bps + advance2_bps`), not `total_amount`, and that `fund_remainder` is the only other call that moves funds in. This distinguishes the current design from prior versions that escrowed the full amount at lock. |
| 1.3 | Deadline boundary convention documented | **Done** | Module doc §"All absolute deadlines" and `Commitment::claim_window_secs` field comment both state the inclusive/exclusive boundary rule and cite the pinning tests (`#112–#115`, `deadline_boundary.rs`). |
| 1.4 | Advance-tranche resolution invariant documented | **Done** | `settle`'s doc comment explains why both tranches must be resolved before `settle` runs and what the earlier design (silent sweep) got wrong. `Error::TrancheUnresolved` has a cross-reference to `settle`'s doc. |
| 1.5 | Token-balance invariant documented | **Done** | `initialize`'s doc comment §"A commitment abandoned at Draft forever (#96)" proves the contract balance is zero before `lock`; `cancel` and `reclaim_on_nondelivery` are both documented as balance-based sweeps, correct regardless of claim history. |
| 1.6 | Oracle/FX one-time-resolution invariant documented | **Done** | Module doc §"One-time and computed exactly once" and `resolve_fx_shortfall`'s doc comment explain the no-re-resolve rule and why re-resolving at a different rate would let either side game the number. `Error::FxAlreadyResolved` cross-references this. |
| 1.7 | Dispute freeze semantics documented | **Done** | `flag_dispute`'s doc comment and module doc §`flag_dispute` both state that `Disputed` blocks every other state-changing call by construction (none of them accept `Disputed` as a required status), and that `resolve_dispute` / `expire_dispute_window` restore the exact pre-dispute status. |
| 1.8 | Known limitation: dispute does not pause other deadlines | **Done** | `flag_dispute`'s doc comment §"Known limitation" explicitly names this gap, explains why it was not fixed, and states when to revisit. |
| 1.9 | Storage-lifetime invariant documented | **Done** | `docs/storage-ttl.md` and `bump_instance_ttl`'s doc comment cover the one-entry/one-TTL shape, the re-rent threshold, residual expiry risk, and mitigations. |
| 1.10 | `bps_amount` overflow invariant documented | **Done** | `MAX_TOTAL_AMOUNT` and `Error::AmountTooLarge` comments explain why `initialize` enforces the upper bound so every `bps_amount` call site is overflow-free by construction. |

---

## 2. Threat Model

A written record of the threats considered, the attack surfaces identified,
and the mitigations in place.

| # | Item | Status | Notes |
|---|------|--------|-------|
| 2.1 | Threat model document exists | **Done** | `docs/threat-model.md` is present. |
| 2.2 | Permissionless-release griefing covered | **Done** | `docs/permissionless-release-review.md` and `contracts/escrow/tests/permissionless_release_review.rs` cover the `release_advance_*` attack surface; `release_advance_1`'s doc comment §"Security-reviewed on the record (#2)" summarises the finding inline. |
| 2.3 | Dispute front-running and stalling covered | **Done** | `docs/dispute-frontrunning.md` and `docs/dispute-stalling.md` both exist. |
| 2.4 | Griefing-and-inaction analysis documented | **Done** | `docs/griefing-and-inaction.md` exists and is cross-referenced by `available_actions`'s doc comment. |
| 2.5 | Authorization matrix documented | **Done** | `docs/authorization-matrix.md` lists every public function alongside its `require_auth()` rules. The `AuthParty` enum and `available_actions` provide the machine-readable equivalent. |
| 2.6 | Oracle staleness / unavailability failure modes documented | **Done** | `docs/reflector-decimals-assumption.md` covers the `decimals()` trust assumption and its worst-case silent rescale. `refresh_fx_quote_status`'s doc comment covers the stale-quote / host-failure distinction. |
| 2.7 | Checks-effects-interactions pattern applied and documented | **Done** | Every token transfer in the contract is preceded by a state write; `lock`'s doc comment explains the CEI rationale and notes it is defensive hardening against future token types, not a known exploit on the current SAC/native token. |
| 2.8 | Address-equality audit documented | **Done** | `docs/address-equality-audit.md` and `tests/address_equality.rs` (#122, #139, #140) cover the `require_once_each` deduplication logic. |

---

## 3. Test Coverage Summary

| # | Item | Status | Notes |
|---|------|--------|-------|
| 3.1 | Test coverage summary document exists | **Partial** | `docs/mutation-testing.md` documents mutation-testing results. A single consolidated coverage summary (line/branch coverage percentages, test count by category) has not been written separately; coverage data is spread across `docs/mutation-testing.md` and inline test file comments. |
| 3.2 | Deadline boundary tests exist and are pinned | **Done** | `contracts/escrow/tests/deadline_boundary.rs` covers issues #112–#115 — the four deadline pairs each with an at-deadline and strictly-after-deadline case. |
| 3.3 | Permissionless-release security tests exist | **Done** | `contracts/escrow/tests/permissionless_release_review.rs` covers all scenarios from `docs/permissionless-release-review.md`. |
| 3.4 | Auth-deduplication tests exist | **Done** | `tests/address_equality.rs` covers issues #139/#140 with real `MockAuth` entries rather than `mock_all_auths()`. |
| 3.5 | Oracle / FX integration tests exist | **Done** | Oracle paths (stale quote, missing asset, decimals change) are covered; the `a_plausible_decimals_change_silently_rescales_resolve_fx_shortfall` test is explicitly named in `resolve_fx_shortfall`'s doc comment. |
| 3.6 | Full lifecycle integration tests exist | **Done** | The test suite covers the happy path through every status transition, plus partial-delivery and shortfall scenarios. |
| 3.7 | Mutation testing results documented | **Done** | `docs/mutation-testing.md` exists with per-mutant findings. |
| 3.8 | Coverage gaps identified and tracked | **Partial** | `docs/mutation-testing.md` notes surviving mutants; no formal issue is open for each gap yet. Recommend opening issues for each surviving mutant before audit begins. |

---

## 4. Known Limitations

Explicit, written acknowledgements of design constraints, non-goals, and
deferred work that an auditor should know about upfront.

| # | Item | Status | Notes |
|---|------|--------|-------|
| 4.1 | Dispute does not pause other deadlines | **Done** | Documented in `flag_dispute`'s doc comment §"Known limitation". Workaround: parties can call `cancel` after `resolve_dispute` if a deadline lapsed during the freeze. |
| 4.2 | Reflector NGN quote not currently available on testnet | **Done** | Module doc §`oracle_rate` notes NGN is unquoted by Reflector's testnet fiat oracle (verified via live `assets()` call). Tests use GBP as a proxy; NGN activates the day Reflector adds it with no code change needed. |
| 4.3 | `decimals()` assumed immutable | **Done** | `docs/reflector-decimals-assumption.md` documents the assumption, the SEP-40 basis for it, and the pinned test that shows the worst-case silent-rescale failure mode. |
| 4.4 | Allocation is record-only in v1 (no on-chain pro-rata payout) | **Done** | `set_allocation`'s doc comment and module doc §`set_allocation` both state this is Transparency Ladder Rung 1, by PRD §4.9's own stated default — not an open design question. |
| 4.5 | Storage lifetime: dormant commitment may archive if never called | **Done** | `docs/storage-ttl.md` documents residual expiry risk and the `stellar contract extend`/`restore` + watcher mitigations. |
| 4.6 | Buyer-position reassignment (`reassign_buyer`) is not a market | **Done** | `reassign_buyer`'s doc comment explicitly states no order book, no price discovery — just a novation. |
| 4.7 | FX option (b) selected; options (a) and (c) not built | **Done** | Module doc §"PRD §4.2's option (b)" states the product decision explicitly. |
| 4.8 | `cancel` from `Draft` refunds zero (no funds ever escrowed) | **Done** | `initialize`'s doc comment §"A commitment abandoned at Draft forever (#96)" explains this and why it is not a loss of funds. |

---

## 5. Deployment History

| # | Item | Status | Notes |
|---|------|--------|-------|
| 5.1 | Testnet deployment addresses recorded | **Not done** | No testnet contract addresses have been committed to `docs/`. Recommend adding a `docs/deployments.md` with network, contract address, deploy transaction hash, and deployer address for each deployment before the audit engagement begins. |
| 5.2 | Deployment runbook exists | **Done** | `docs/migration-runbook.md` covers the deploy and upgrade procedure. |
| 5.3 | WASM hash pinning for audited build | **Not done** | The audited WASM binary's SHA-256 hash has not been committed alongside the source. Recommend computing `sha256sum target/wasm32-unknown-unknown/release/escrow.wasm` for the audited build and committing it to `docs/deployments.md`. |
| 5.4 | Constructor / initialization batching documented | **Done** | `docs/constructor-batching.md` covers how `initialize` and the initial `lock` can be batched in one transaction. |
| 5.5 | Upgrade / migration strategy documented | **Done** | `docs/versioning.md` and `docs/migration-runbook.md` both exist. |
| 5.6 | Audit-scope commit SHA pinned | **Not done** | The exact commit SHA and branch to audit has not been recorded in a `docs/deployments.md` or equivalent. Recommend doing this before handing the repo to auditors. |

---

## Summary

| Category | Done | Partial | Not done |
|----------|------|---------|----------|
| 1. Invariants | 10 | 0 | 0 |
| 2. Threat model | 8 | 0 | 0 |
| 3. Test coverage | 6 | 2 | 0 |
| 4. Known limitations | 8 | 0 | 0 |
| 5. Deployment history | 3 | 0 | 3 |
| **Total** | **35** | **2** | **3** |

**Recommended actions before audit engagement begins:**

1. Create `docs/deployments.md` with testnet contract addresses, deploy tx hashes, deployer addresses, and the audited build's WASM SHA-256.
2. Pin the audit-scope commit SHA in `docs/deployments.md`.
3. Open issues for each surviving mutant from `docs/mutation-testing.md` and confirm triage before handing to auditors.
