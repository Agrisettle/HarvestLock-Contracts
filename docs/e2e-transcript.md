# E2E Testnet Transcript Format & Lifecycle Automation

This document describes the automated end-to-end (E2E) lifecycle verification script (`scripts/e2e.sh`), the deployment helper (`scripts/deploy.sh`), and the JSON transcript schema recording real on-chain transaction hashes.

## Overview

Issue #286 automates live lifecycle execution across three core commitment paths:
1. **Happy Path (Two-Phase Funding)**: Draft -> Locked -> Advance1Released -> ClaimAdvance1 -> CheckpointPassed -> Advance2Released -> ClaimAdvance2 -> ReadyForDelivery -> FundRemainder -> Delivered -> Settled.
2. **Buyer Default**: Remainder window passes unfunded -> `expire_remainder_window` sweeps the contract's remaining balance to the cooperative -> Defaulted.
3. **Seller Non-Delivery**: Cooperative takes no action past the delivery deadline -> buyer's `reclaim_on_nondelivery` refunds the deposit -> Forfeited.

After each state-changing invocation, the runner queries and asserts on-chain status via `get_status` and token balances on the asset contract (SAC), producing a verifiable JSON transcript citing real transaction hashes.

## Running the E2E Suite

```bash
# Run all three scenarios on testnet:
./scripts/e2e.sh

# Run on a custom network with an existing token SAC:
./scripts/e2e.sh --network testnet --token <TOKEN_CONTRACT_ID>

# Run a specific scenario:
./scripts/e2e.sh --scenario happy
./scripts/e2e.sh --scenario default
./scripts/e2e.sh --scenario non-delivery
```

## JSON Transcript Schema

Transcripts are written to `transcripts/e2e-testnet-<timestamp>.json` and symlinked/copied to `transcripts/e2e-testnet-latest.json`.

### Top-Level Structure

```json
{
  "network": "testnet",
  "timestamp": "20261124T102220Z",
  "token": "CAFC34O2KANLBWHYIPDT6MHPOETAEM7VXYSLHASJRUHEWUDMHNJW6SDL",
  "identities": {
    "buyer": "e2e-buyer (GD2LJAGC...)",
    "cooperative": "e2e-coop (GD5QIL3G...)",
    "warehouse": "e2e-wh (GCREIWC...)",
    "stranger": "e2e-stranger (GDBKG4J...)",
    "deployer": "e2e-deployer (GAY5WPJ...)"
  },
  "scenarios": [
    ...
  ]
}
```

### Scenario Object

Each element in `scenarios` contains:
- `name`: Scenario identifier (`"happy_path"`, `"buyer_default"`, `"seller_nondelivery"`).
- `contract_id`: Deployed escrow contract ID (`C...`).
- `deploy_tx_hash`: On-chain transaction hash for contract deployment.
- `wasm_hash`: Hash of the installed contract WASM.
- `parameters`: Commitment terms (amounts, basis points, window durations).
- `steps`: Array of chronological step records.

### Step Record Object

```json
{
  "step": 2,
  "action": "lock",
  "caller": "e2e-buyer",
  "args": [],
  "tx_hash": "9406d21a555a6c4520d0a58d291c4d04ad5aa156f3fe2c793765abde138309b0",
  "result": "ok",
  "status_after": "Locked",
  "balances_after": {
    "contract": 35000000,
    "buyer": 2090000000,
    "cooperative": 130000000
  }
}
```

## Fixture Provenance

The real transaction hashes and outcomes from recorded transcripts are transcribed into `.snap` regression fixtures in `contracts/escrow/tests/fixtures/`:
- `deployment-13-real-hashes-happy-path.snap`
- `deployment-13-real-hashes-buyer-default.snap`
- `deployment-13-real-hashes-seller-non-delivery.snap`

Each fixture's `provenance =` explicitly cites the on-chain contract ID, WASM hash, and individual transaction hashes verifiable on [stellar.expert](https://stellar.expert/explorer/testnet). Replayed automatically by `contracts/escrow/tests/snapshot_replay.rs`.
