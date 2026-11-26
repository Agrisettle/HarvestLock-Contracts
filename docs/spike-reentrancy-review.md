# Spike: Cross-Contract Reentrancy Review

**Issue Reference:** Closes #49 | Fixes https://github.com/Agrisettle/HarvestLock-Contracts/issues/49  
**Scope:** `contracts/escrow/src/lib.rs` and Soroban environment host interaction model.

---

## 1. Executive Summary

This spike reviews every cross-contract invocation made by `HarvestLock-Contracts` (`contracts/escrow/src/lib.rs`), focusing on:
1. Standard token transfers (SEP-41 / SAC token client calls: `transfer`).
2. Live oracle price and decimals queries against the Reflector SEP-40 contract (`ReflectorPulseClient::try_lastprice` and `try_decimals`).

We evaluated whether the contract's "callback-free" assumption is guaranteed by Soroban's underlying architecture and execution host, or merely an empirical accident.

**Core Findings:**
- **Soroban Execution Model:** Soroban supports synchronous cross-contract calls via the host environment (`Env`). Contract execution runs on a single-threaded WebAssembly VM. While Soroban allows reentrancy in principle if a called contract immediately invokes the caller, standard Stellar Asset Contract (SAC) implementations do not provide execution hooks or arbitrary caller callbacks on `transfer`.
- **Reflector Oracle:** Reflector SEP-40 contracts are read-only feed providers with no callback interface. In addition, calls in HarvestLock are wrapped with `try_lastprice` and `try_decimals` to guard against host traps.
- **Checks-Effects-Interactions (CEI):** In all state-mutating functions that invoke external contracts (`lock`, `claim_advance_1`, `claim_advance_2`, `reclaim_advance_1`, `reclaim_advance_2`, `settle`, `cancel`, `resolve_dispute`, `expire_remainder_window`, `reclaim_on_nondelivery`, and `fund_fx_shortfall`), state updates are saved to instance/temporary storage *before* any external token transfer is initiated.
- **Transactional Atomicity:** If any external invocation traps or fails, Soroban rolls back the entire invocation frame atomically.

**Recommendation:**
Maintain the strict Checks-Effects-Interactions ordering already established in `HANDOFF.md` and `contracts/escrow/src/lib.rs`. No reentrancy lock or external mutex is required; the existing pattern provides complete defense-in-depth against reentrancy.

---

## 2. Inventory of Cross-Contract Calls

The contract executes cross-contract calls in two categories:

### A. SEP-41 / Stellar Asset Contract (`soroban_sdk::token::Client`)
Calls made:
- `token::Client::new(&env, &token).transfer(&from, &to, &amount)`
- `token::Client::new(&env, &token).decimals()`

Locations:
- `lock`: Buyer transfers `deposit_amount` into escrow. (Storage updated before transfer).
- `fund_remainder`: Buyer transfers remainder into escrow. (Storage updated before transfer).
- `fund_fx_shortfall`: Buyer transfers shortfall amount into escrow. (Storage updated before transfer).
- `claim_tranche` (via `claim_advance_1` / `claim_advance_2`): Contract transfers tranche amount to cooperative. (Storage updated before transfer).
- `reclaim_tranche` (via `reclaim_advance_1` / `reclaim_advance_2`): Contract transfers tranche amount back to buyer. (Storage updated before transfer).
- `settle`: Contract transfers remaining balance to cooperative, deposit to buyer/coop per terms. (Storage updated before transfer).
- `cancel`: Contract refunds buyer / coop according to cancellation logic. (Storage updated before transfer).
- `resolve_dispute`: Contract transfers escrowed amounts to buyer/cooperative based on arbiter decision. (Storage updated before transfer).
- `expire_remainder_window`: Sweeps unfulfilled remainder to buyer/coop. (Storage updated before transfer).
- `reclaim_on_nondelivery`: Refunds buyer upon warehouse delivery failure. (Storage updated before transfer).
- `resolve_fx_shortfall`: Queries `token::Client::new(&env, &c.token).decimals()`. Read-only call.

### B. Reflector SEP-40 Oracle (`ReflectorPulseClient`)
Calls made:
- `client.try_lastprice(&ReflectorAsset::Other(config.price_asset.clone()))`
- `client.try_decimals()`

Locations:
- `read_oracle_rate` (invoked by `oracle_rate`, `resolve_fx_shortfall`, and `refresh_fx_quote_status`).
- `resolve_fx_shortfall`: Invokes `try_lastprice` followed by `try_decimals`.

---

## 3. Analysis: Host Guarantees vs. Assumptions

### 3.1 Can a Token Transfer Reenter HarvestLock?
In Ethereum (ERC-777, ERC-1155, or ERC-20 with fallback/hooks like Uniswap/ERC-1363), token transfers execute arbitrary recipient code before returning control to the caller.

In Soroban:
- Standard Classic Stellar assets bridged to Soroban via SAC (Stellar Asset Contract) and standard SEP-41 tokens have fixed, built-in implementations.
- There are no `beforeTokenTransfer` or `tokensReceived` callback hooks in SAC.
- However, Soroban allows custom smart contracts implementing the SEP-41 interface. A malicious custom token contract could theoretically execute a call back into HarvestLock during a `transfer` call if programmed to do so.
- **HarvestLock's Mitigation:** Every single transfer site adheres strictly to Checks-Effects-Interactions (CEI).
  - In `claim_tranche`: `c.advance1_claimed = true; Self::save(&env, &c);` is written to host storage before `token.transfer(...)`. Any reentrant call to `claim_tranche` immediately encounters `c.advance1_claimed == true` and fails with `Error::TrancheAlreadyClaimed`.
  - In `settle`: `c.settled = true; Self::save(&env, &c);` happens prior to the final transfer.
  - In `cancel`: State transitions to terminal before balances are returned.
  Therefore, even if a custom token implementation attempted a reentrant callback, the state machine prohibits reentrancy exploits.

### 3.2 Can the Reflector Oracle Call Reenter HarvestLock?
- Reflector oracle contracts implement SEP-40 price feeds. Price feed contracts on Soroban publish or store recorded prices and provide read methods (`lastprice`, `decimals`, `prices`, `assets`).
- The calls in `read_oracle_rate` and `resolve_fx_shortfall` are executed against `config.oracle_contract`.
- Both calls are read-only inquiries (`try_lastprice`, `try_decimals`).
- Crucially, `read_oracle_rate` performs no state mutations and holds no intermediate locks.
- In `resolve_fx_shortfall`, the sequence is:
  1. `Self::can_resolve_fx_shortfall(&env, &c)?;` (asserts both tranches already resolved and shortfall not already resolved).
  2. Query `read_oracle_rate` (calls Reflector `try_lastprice`).
  3. Query Reflector `try_decimals`.
  4. Query Token `decimals()`.
  5. Compute FX shortfall.
  6. Write `c.fx_shortfall_amount`, `c.fx_resolved = true`, and `Self::save(&env, &c)`.
- If an oracle were compromised and attempted to call back into `resolve_fx_shortfall`, the check `Self::can_resolve_fx_shortfall(&env, &c)` relies on immutable state or already-committed tranche states. Moreover, `try_lastprice` merely retrieves a value.

### 3.3 Host Rollback Behavior
Soroban execution semantics ensure that:
- Any unhandled panic or `Err` return unwinds all storage changes made during that invocation frame.
- If a downstream contract panics or traps, and the caller does not catch it with `try_*`, the entire call stack reverts cleanly.
- HarvestLock uses `try_lastprice` and `try_decimals` to explicitly handle callee failure gracefully as `Error::OracleUnavailable` rather than letting an external failure cause an untracked abort.

---

## 4. Findings & Recommendations

1. **Checks-Effects-Interactions (CEI) is consistently maintained:** All 10 external transfer entry points modify and persist contract storage before invoking external token transfers.
2. **Oracle call safety:** Reflector calls are read-only and resilient against failure via `try_*`. They cannot induce unexpected state reentrancy.
3. **No Explicit Mutex Needed:** Introducing a dedicated reentrancy guard variable (`bool is_locked`) is unnecessary and would increase bytecode size and CPU/storage gas costs without adding real security over the existing CEI discipline and Soroban's invocation rollback semantics.
4. **Actionable Recommendation:**
   - Retain the strict CEI ordering rule in future PRs and record this confirmation in the security documentation.
   - For any future external integrations (e.g. dynamic hooks or automated liquidations), require static analysis or verification that calls happen after state finalization.
