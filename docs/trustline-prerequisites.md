# Stellar Asset Contract (SAC) & Trustline Prerequisites

This document outlines the asset custody requirements and per-role trustline
prerequisites when running the HarvestLock Escrow Contract on Stellar with
issued classic assets (e.g. USDC, HLUSD) rather than native XLM.

---

## 1. Native XLM SAC vs. Issued Classic Asset SAC

All previous benchmark walkthroughs (such as those in [`docs/gas-costs.md`](./gas-costs.md))
standardized on the **native XLM SAC** (`CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC`).

While convenient for resource-metering sessions, using native XLM hides a critical
protocol mechanic:

- **Native XLM**: Balances are held directly in every Stellar account's base account
  record (`AccountEntry`). No trustlines are needed or allowed for native XLM.
- **Issued Classic Assets (e.g., USDC, EURC, pilot tokens like HLUSD)**: Balances
  are held in **trustline entries** (`TrustLineEntry`). Under Stellar protocol rules
  (and SEP-41), an account **cannot receive, hold, or be paid** any amount of an
  issued asset unless that account has explicitly established a trustline to the
  asset's issuer (`stellar tx new change-trust`).

In a real pilot or production deployment, contracts settle in an issued stablecoin
or asset, so counterparties must satisfy the trustline prerequisites before
transactions can proceed.

---

## 2. Per-Role Trustline Prerequisites

| Role | Address Type | Trustline Required? | Why / Critical Operations |
|---|---|:---:|---|
| **Buyer** (`buyer`) | Stellar Account (`G...`) | **YES** | • Must hold trustline to receive minted tokens from issuer.<br>• Funds the escrow deposit in `lock()` (SEP-41 `transfer`).<br>• Must keep trustline to receive refunds on `cancel()`, `reclaim_advance_1()`, `reclaim_advance_2()`, `reclaim_on_nondelivery()`, or surplus refunds at `settle()`. |
| **Cooperative** (`cooperative`) | Stellar Account (`G...`) | **YES** | • Must hold trustline before claiming advances or settlement.<br>• Receives payouts in `claim_advance_1()`, `claim_advance_2()`, and `settle()`.<br>• Receives forfeited funds in `expire_remainder_window()` and `expire_fx_shortfall_window()`.<br>• **Without a trustline, any claim or settlement transfer fails with a host error, blocking fund release.** |
| **Escrow Contract Instance** | Soroban Contract (`C...`) | **NO** | • Contract instances are Soroban contracts, not classic Stellar accounts.<br>• The SAC tracks contract balances in Soroban contract storage entries.<br>• No classic trustline or 0.5 XLM base reserve is required for contract instances. |
| **Warehouse Operator** (`warehouse_operator`) | Stellar Account (`G...`) | **NO** | • Pure attestation and dispute gating role (`mark_checkpoint`, `confirm_delivery`, `flag_dispute`).<br>• Moves zero settlement tokens. |
| **Deployer / Stranger** | Stellar Account (`G...`) | **NO** | • Deploys instance and can trigger permissionless clock calls (`release_advance_1`, `release_advance_2`) or timeout sweeps.<br>• Moves zero settlement tokens. |
| **Asset Issuer** (`issuer`) | Stellar Account (`G...`) | **NO** | • Issues the asset; cannot hold a trustline to its own issued asset.<br>• Distributes initial balances by paying accounts that hold trustlines. |

> [!IMPORTANT]
> **Cooperative Trustline Failure Mode:**
> If a commitment is initialized with an issued asset SAC and the cooperative has
> not established a trustline for that asset, `lock()` and `release_advance_1()`
> will still succeed (because they do not transfer funds to the cooperative). However,
> the moment the cooperative calls `claim_advance_1()`, the SAC cross-contract
> `transfer(from: contract, to: cooperative, amount)` call will fail at the token
> contract layer with a missing trustline error, halting the transaction and unwinding
> all state changes. The cooperative must configure its trustline before calling
> `claim_advance_*` or `settle`.

---

## 3. Issuer Flags & Threat Model Considerations

Stellar classic asset issuers can set authorization flags on the issuing account via
`stellar tx new set-options`:

1. **`AUTH_REVOCABLE` (`0x2`)**:
   Allows the issuing account to freeze an existing trustline, preventing the
   account from transferring or receiving the asset.
2. **`AUTH_CLAWBACK_ENABLED` (`0x8`)**:
   Allows the issuing account to claw back (burn) the asset from any account's
   balance or claimable balance.

### Threat Model Alignment
As documented in [`docs/threat-model.md`](./threat-model.md) (§Actor Table & Threat 30):
- **Contract Isolation**: The escrow contract does not hold issuer keys and has no
  admin role. It relies on standard SEP-41 transfer semantics.
- **Counterparty Risk**: If an issued asset has clawback or revocable flags set, the
  external issuer possesses protocol-level power over non-escrowed account balances.
  Parties entering into a commitment must verify the issuer's flags and trust assumptions
  prior to signing `initialize`.

---

## 4. Automation: `scripts/test-asset.sh`

The [`scripts/test-asset.sh`](../scripts/test-asset.sh) script automates the complete
lifecycle for local and testnet verification:

1. Generates and funds the issuer, buyer, and cooperative testnet identities (if not already existing).
2. Optionally configures `AUTH_REVOCABLE` and `AUTH_CLAWBACK_ENABLED` on the issuer.
3. Deploys the Soroban Asset Contract (SAC) for the classic asset using `stellar contract asset deploy`.
4. Establishes the required trustlines for both the `buyer` and `cooperative`.
5. Mints the initial settlement balance to the buyer.
6. Prints the SAC Contract ID to be passed as `--token` to `initialize`.

### Usage

```bash
# Basic setup on testnet (issues HLUSD with 7 decimals):
scripts/test-asset.sh

# Quiet mode (prints ONLY the SAC contract ID):
TOKEN=$(scripts/test-asset.sh --quiet)

# With issuer flags enabled:
scripts/test-asset.sh --asset-code HLUSD --all-flags --amount 100000000
```

---

## 5. Live Testnet Walkthrough with an Issued Asset

Below is a complete, live-verified testnet sequence demonstrating deployment,
initialization with an issued asset SAC, deposit locking, advance release, and
successful advance claiming by the cooperative:

```bash
# 1. Issue asset, set up trustlines, and obtain SAC contract ID
TOKEN=$(scripts/test-asset.sh --asset-code HLUSD --quiet)

BUYER=$(stellar keys address buyer)
COOP=$(stellar keys address cooperative)
WAREHOUSE=$(stellar keys address warehouse)

# 2. Build and upload escrow contract WASM
stellar contract build --locked
WASM_HASH=$(stellar contract upload \
  --wasm target/wasm32v1-none/release/harvestlock_escrow.wasm \
  --source-account deployer --network testnet)

# 3. Deploy escrow instance
ESCROW_ID=$(stellar contract deploy \
  --wasm-hash "$WASM_HASH" \
  --source-account deployer --network testnet)

# 4. Initialize commitment using the issued asset SAC as --token
stellar contract invoke --id "$ESCROW_ID" --source-account buyer --network testnet -- \
  initialize \
  --buyer "$BUYER" --cooperative "$COOP" --warehouse-operator "$WAREHOUSE" \
  --token "$TOKEN" \
  --total-amount 100000000 --advance1-bps 1500 --advance2-bps 2000 \
  --claim-window-secs 3600 --remainder-window-secs 3600 --delivery-window-secs 86400 \
  --contracted-quantity 1000 --grade-price-bps '[10000,9000,7500]' --oracle-config null

# 5. Lock commitment (buyer transfers deposit to contract)
stellar contract invoke --id "$ESCROW_ID" --source-account buyer --network testnet -- lock

# 6. Release advance 1 (moves state to Advance1Released)
stellar contract invoke --id "$ESCROW_ID" --source-account deployer --network testnet -- release_advance_1

# 7. Cooperative claims advance 1 (transfers 15,000,000 stroops to cooperative)
#    Succeeds because cooperative holds the required trustline!
stellar contract invoke --id "$ESCROW_ID" --source-account cooperative --network testnet -- claim_advance_1

# 8. Verify cooperative balance on the SAC
stellar contract invoke --id "$TOKEN" --source-account cooperative --network testnet -- balance --id "$COOP"
# → "15000000"
```
