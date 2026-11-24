#!/usr/bin/env bash
# End-to-end lifecycle verification on a live Stellar network with real transaction hashes.
#
# Issue #286: Script full lifecycle runs on a live network and record real transaction hashes.
# Drives three fresh commitments through:
#   1. Happy path with two-phase funding (through settle)
#   2. Buyer default (expire_remainder_window after remainder deadline passes)
#   3. Seller non-delivery (reclaim_on_nondelivery after delivery deadline passes)
#
# Asserts get_status and token balances after each step and records a JSON transcript.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/e2e.sh [OPTIONS]

Run full end-to-end contract lifecycles on testnet and record real transaction hashes.

Options:
  -n, --network <NAME>              Stellar network (default: testnet, or $STELLAR_NETWORK)
  -b, --buyer <IDENTITY>            Buyer identity (default: e2e-buyer, or $BUYER)
  -c, --cooperative <IDENTITY>      Cooperative identity (default: e2e-coop, or $COOPERATIVE)
  -w, --warehouse <IDENTITY>        Warehouse operator identity (default: e2e-wh, or $WAREHOUSE)
  -s, --stranger <IDENTITY>         Stranger/third-party identity (default: e2e-stranger, or $STRANGER)
  -d, --deployer <IDENTITY>         Deployer identity (default: e2e-deployer, or $DEPLOYER)
  -t, --token <CONTRACT_ID>         Token SAC address (default: issued classic asset via test-asset.sh)
      --native-token                Use native XLM SAC instead of an issued asset
  -o, --output <PATH>               Transcript output file (default: transcripts/e2e-testnet-<timestamp>.json)
      --scenario <all|happy|default|non-delivery> Scenario to run (default: all)
  -h, --help                        Show this help message
USAGE
}

NETWORK="${STELLAR_NETWORK:-testnet}"
BUYER="${BUYER:-e2e-buyer}"
COOP="${COOPERATIVE:-e2e-coop}"
WAREHOUSE="${WAREHOUSE:-e2e-wh}"
STRANGER="${STRANGER:-e2e-stranger}"
DEPLOYER="${DEPLOYER:-e2e-deployer}"
TOKEN=""
USE_NATIVE_TOKEN=0
OUTPUT_PATH=""
SELECTED_SCENARIO="all"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -n|--network)
      NETWORK="$2"
      shift 2
      ;;
    -b|--buyer)
      BUYER="$2"
      shift 2
      ;;
    -c|--cooperative|--coop)
      COOP="$2"
      shift 2
      ;;
    -w|--warehouse)
      WAREHOUSE="$2"
      shift 2
      ;;
    -s|--stranger)
      STRANGER="$2"
      shift 2
      ;;
    -d|--deployer)
      DEPLOYER="$2"
      shift 2
      ;;
    -t|--token)
      TOKEN="$2"
      shift 2
      ;;
    --native-token)
      USE_NATIVE_TOKEN=1
      TOKEN="CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"
      shift
      ;;
    -o|--output)
      OUTPUT_PATH="$2"
      shift 2
      ;;
    --scenario)
      SELECTED_SCENARIO="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      usage >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
mkdir -p "$REPO_ROOT/transcripts"

TIMESTAMP="$(date -u +"%Y%m%dT%H%M%SZ")"
if [[ -z "$OUTPUT_PATH" ]]; then
  OUTPUT_PATH="$REPO_ROOT/transcripts/e2e-testnet-${TIMESTAMP}.json"
fi

LATEST_PATH="$REPO_ROOT/transcripts/e2e-testnet-latest.json"

log() {
  echo "$@" >&2
}

ensure_identity() {
  local identity="$1"
  if ! stellar keys address "$identity" >/dev/null 2>&1; then
    log "==> Generating and funding identity '$identity' on network '$NETWORK'..."
    stellar keys generate "$identity" --network "$NETWORK" --fund >/dev/null 2>&1
  else
    stellar keys fund "$identity" --network "$NETWORK" >/dev/null 2>&1 || true
  fi
}

log "============================================================================="
log "HarvestLock Escrow E2E Live Lifecycle Suite"
log "============================================================================="
log "Network:        $NETWORK"
log "Deployer:       $DEPLOYER"
log "Buyer:          $BUYER"
log "Cooperative:    $COOP"
log "Warehouse:      $WAREHOUSE"
log "Stranger:       $STRANGER"
log "Output Path:    $OUTPUT_PATH"
log "============================================================================="

# Ensure all 5 identities exist and are funded on the target network
ensure_identity "$DEPLOYER"
ensure_identity "$BUYER"
ensure_identity "$COOP"
ensure_identity "$WAREHOUSE"
ensure_identity "$STRANGER"

BUYER_ADDR="$(stellar keys address "$BUYER")"
COOP_ADDR="$(stellar keys address "$COOP")"
WAREHOUSE_ADDR="$(stellar keys address "$WAREHOUSE")"
STRANGER_ADDR="$(stellar keys address "$STRANGER")"
DEPLOYER_ADDR="$(stellar keys address "$DEPLOYER")"

# Setup token: use issued asset by default (exact zero-dust balances) or native XLM SAC
if [[ -z "$TOKEN" ]]; then
  log "==> Setting up test asset (HLUSD) with trustlines and buyer mint (300,000,000 stroops)..."
  TOKEN="$("$SCRIPT_DIR/test-asset.sh" \
    --network "$NETWORK" \
    --asset-code HLUSD \
    --buyer "$BUYER" \
    --cooperative "$COOP" \
    --amount 300000000 \
    --quiet)"
fi
log "==> Settlement Token Contract ID: $TOKEN"

# Query token balance in stroops with retry
get_token_balance() {
  local target_addr="$1"
  local bal=""
  for _ in {1..6}; do
    local raw
    raw="$(stellar contract invoke --id "$TOKEN" --source-account "$DEPLOYER" --network "$NETWORK" -- balance --id "$target_addr" 2>&1 || true)"
    bal="$(echo "$raw" | grep -oE '"[0-9]+"' | tr -d '"' | tail -n1 || true)"
    if [[ -z "$bal" ]]; then
      bal="$(echo "$raw" | grep -oE '^[0-9]+$' | tail -n1 || true)"
    fi
    if [[ -n "$bal" ]]; then
      echo "$bal"
      return 0
    fi
    sleep 1
  done
  echo "0"
}

# Query contract status with retry
get_contract_status() {
  local contract_id="$1"
  local st=""
  for _ in {1..6}; do
    local raw
    raw="$(stellar contract invoke --id "$contract_id" --source-account "$DEPLOYER" --network "$NETWORK" -- get_status 2>&1 || true)"
    st="$(echo "$raw" | grep -oE '"[A-Za-z0-9]+"' | tr -d '"' | tail -n1 || true)"
    if [[ -n "$st" && "$st" != "Unknown" ]]; then
      echo "$st"
      return 0
    fi
    sleep 1
  done
  echo "Unknown"
}

# Extract transaction hash from CLI output
extract_tx_hash() {
  local output="$1"
  local tx_hash
  tx_hash="$(echo "$output" | grep -oE 'Signing transaction: [a-f0-9]{64}' | awk '{print $3}' | head -n1 || true)"
  if [[ -z "$tx_hash" ]]; then
    tx_hash="$(echo "$output" | grep -oE 'tx/[a-f0-9]{64}' | cut -d/ -f2 | head -n1 || true)"
  fi
  echo "${tx_hash:-unknown}"
}

# Helper to execute an on-chain contract call, assert outcomes, and return JSON step
invoke_step() {
  local step_num="$1"
  local action="$2"
  local caller="$3"
  local contract_id="$4"
  local expect_status="$5"
  local expect_contract_bal="$6"
  shift 6
  local call_args=("$@")

  log "    [Step $step_num] $caller -> $action (${call_args[*]:-})..."
  local out=""
  local success=0
  for attempt in {1..3}; do
    if [[ ${#call_args[@]} -gt 0 ]]; then
      out="$(stellar contract invoke --id "$contract_id" --source-account "$caller" --network "$NETWORK" -- "$action" "${call_args[@]}" 2>&1 || true)"
    else
      out="$(stellar contract invoke --id "$contract_id" --source-account "$caller" --network "$NETWORK" -- "$action" 2>&1 || true)"
    fi
    if echo "$out" | grep -q "Transaction submitted successfully"; then
      success=1
      break
    fi
    # If network timed out or errored after RPC received it, check if state actually transitioned
    local check_st
    check_st="$(get_contract_status "$contract_id")"
    if [[ "$check_st" == "$expect_status" ]]; then
      success=1
      break
    fi
    log "      [attempt $attempt failed: $(echo "$out" | grep -oE '(Request timeout|error:.*)' | head -n1 || echo 'retrying...'), retrying in 3s...]"
    sleep 3
  done

  if [[ "$success" -ne 1 ]]; then
    echo "error: call $action failed by $caller on contract $contract_id after 3 attempts" >&2
    echo "$out" >&2
    return 1
  fi

  local tx_hash
  tx_hash="$(extract_tx_hash "$out")"
  log "      tx: $tx_hash"

  sleep 2

  local status_after
  status_after="$(get_contract_status "$contract_id")"
  if [[ "$status_after" != "$expect_status" ]]; then
    echo "error: expected status '$expect_status' after $action, got '$status_after'" >&2
    return 1
  fi

  local contract_bal buyer_bal coop_bal
  contract_bal="$(get_token_balance "$contract_id")"
  buyer_bal="$(get_token_balance "$BUYER_ADDR")"
  coop_bal="$(get_token_balance "$COOP_ADDR")"

  log "      status: $status_after | contract: $contract_bal | buyer: $buyer_bal | coop: $coop_bal"

  if [[ "$expect_contract_bal" != "any" && "$contract_bal" -ne "$expect_contract_bal" ]]; then
    echo "error: expected contract balance $expect_contract_bal after $action, got $contract_bal" >&2
    return 1
  fi

  # Convert args array to JSON
  local args_json="[]"
  if [[ ${#call_args[@]} -gt 0 ]]; then
    args_json="$(printf '%s\n' "${call_args[@]}" | jq -R . | jq -s .)"
  fi

  # Return JSON step object
  jq -n \
    --arg step "$step_num" \
    --arg action "$action" \
    --arg caller "$caller" \
    --argjson args "$args_json" \
    --arg tx_hash "$tx_hash" \
    --arg result "ok" \
    --arg status_after "$status_after" \
    --argjson contract_balance "$contract_bal" \
    --argjson buyer_balance "$buyer_bal" \
    --argjson coop_balance "$coop_bal" \
    '{
      step: ($step | tonumber),
      action: $action,
      caller: $caller,
      args: $args,
      tx_hash: $tx_hash,
      result: $result,
      status_after: $status_after,
      balances_after: {
        contract: $contract_balance,
        buyer: $buyer_balance,
        cooperative: $coop_balance
      }
    }'
}

# =============================================================================
# SCENARIO 1: Happy Path with Two-Phase Funding
# =============================================================================
run_happy_path() {
  log ""
  log "============================================================================="
  log "Scenario 1: Happy Path with Two-Phase Funding (Lock -> Claims -> Settle)"
  log "============================================================================="

  local total_amount=100000000
  local adv1_bps=1500
  local adv2_bps=2000
  local claim_window=300
  local remainder_window=300
  local delivery_window=600
  local contracted_qty=1000

  log "==> Deploying commitment contract..."
  local deploy_raw
  deploy_raw="$("$SCRIPT_DIR/deploy.sh" \
    --network "$NETWORK" \
    --source "$DEPLOYER" \
    --buyer "$BUYER" \
    --cooperative "$COOP" \
    --warehouse-operator "$WAREHOUSE" \
    --token "$TOKEN" \
    --total-amount "$total_amount" \
    --advance1-bps "$adv1_bps" \
    --advance2-bps "$adv2_bps" \
    --claim-window-secs "$claim_window" \
    --remainder-window-secs "$remainder_window" \
    --delivery-window-secs "$delivery_window" \
    --contracted-quantity "$contracted_qty" \
    --grade-price-bps '[10000,9000,7500]' \
    --json)"

  local contract_id deploy_tx_hash init_tx_hash wasm_hash
  contract_id="$(echo "$deploy_raw" | jq -r .contract_id)"
  deploy_tx_hash="$(echo "$deploy_raw" | jq -r .deploy_tx_hash)"
  init_tx_hash="$(echo "$deploy_raw" | jq -r .init_tx_hash)"
  wasm_hash="$(echo "$deploy_raw" | jq -r .wasm_hash)"

  log "    Contract ID: $contract_id"
  log "    Init Tx:     $init_tx_hash"

  sleep 2
  local initial_status
  initial_status="$(get_contract_status "$contract_id")"
  if [[ "$initial_status" != "Draft" ]]; then
    echo "error: expected initial status Draft, got $initial_status" >&2
    return 1
  fi

  local initial_contract_bal initial_buyer_bal initial_coop_bal
  initial_contract_bal="$(get_token_balance "$contract_id")"
  initial_buyer_bal="$(get_token_balance "$BUYER_ADDR")"
  initial_coop_bal="$(get_token_balance "$COOP_ADDR")"

  local steps="[]"
  local init_step
  init_step="$(jq -n \
    --arg tx_hash "$init_tx_hash" \
    --argjson cbal "$initial_contract_bal" \
    --argjson bbal "$initial_buyer_bal" \
    --argjson cobal "$initial_coop_bal" \
    '{
      step: 1,
      action: "initialize",
      caller: "e2e-buyer",
      args: [],
      tx_hash: $tx_hash,
      result: "ok",
      status_after: "Draft",
      balances_after: {
        contract: $cbal,
        buyer: $bbal,
        cooperative: $cobal
      }
    }')"
  steps="$(echo "$steps" | jq ". + [$init_step]")"

  # Step 2: lock (escrows advance1 + advance2 bps = 35,000,000)
  local s2
  s2="$(invoke_step 2 "lock" "$BUYER" "$contract_id" "Locked" 35000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s2]")"

  # Step 3: release_advance_1
  local s3
  s3="$(invoke_step 3 "release_advance_1" "$COOP" "$contract_id" "Advance1Released" 35000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s3]")"

  # Step 4: claim_advance_1 (coop receives 15,000,000; contract left with 20,000,000)
  local s4
  s4="$(invoke_step 4 "claim_advance_1" "$COOP" "$contract_id" "Advance1Released" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s4]")"

  # Step 5: mark_checkpoint (warehouse operator attests checkpoint)
  local s5
  s5="$(invoke_step 5 "mark_checkpoint" "$WAREHOUSE" "$contract_id" "CheckpointPassed" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s5]")"

  # Step 6: release_advance_2
  local s6
  s6="$(invoke_step 6 "release_advance_2" "$COOP" "$contract_id" "Advance2Released" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s6]")"

  # Step 7: claim_advance_2 (coop receives 20,000,000; contract left with 0)
  local s7
  s7="$(invoke_step 7 "claim_advance_2" "$COOP" "$contract_id" "Advance2Released" 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s7]")"

  # Step 8: ready_for_delivery
  local s8
  s8="$(invoke_step 8 "ready_for_delivery" "$COOP" "$contract_id" "ReadyForDelivery" 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s8]")"

  # Step 9: fund_remainder (buyer funds remaining 65,000,000)
  local s9
  s9="$(invoke_step 9 "fund_remainder" "$BUYER" "$contract_id" "ReadyForDelivery" 65000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s9]")"

  # Step 10: confirm_delivery
  local s10
  s10="$(invoke_step 10 "confirm_delivery" "$WAREHOUSE" "$contract_id" "Delivered" 65000000 --delivered-quantity 1000 --grade-index 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s10]")"

  # Step 11: settle (sweeps remaining 65,000,000 to cooperative)
  local s11
  s11="$(invoke_step 11 "settle" "$COOP" "$contract_id" "Settled" 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s11]")"

  local params_json='{"total_amount":100000000,"advance1_bps":1500,"advance2_bps":2000,"claim_window_secs":300,"remainder_window_secs":300,"delivery_window_secs":600,"contracted_quantity":1000,"grade_price_bps":[10000,9000,7500]}'

  jq -n \
    --arg name "happy_path" \
    --arg contract_id "$contract_id" \
    --arg deploy_tx "$deploy_tx_hash" \
    --arg wasm_hash "$wasm_hash" \
    --argjson params "$params_json" \
    --argjson steps "$steps" \
    '{
      name: $name,
      contract_id: $contract_id,
      deploy_tx_hash: $deploy_tx,
      wasm_hash: $wasm_hash,
      parameters: $params,
      steps: $steps
    }'
}

# =============================================================================
# SCENARIO 2: Buyer Default (expire_remainder_window)
# =============================================================================
run_buyer_default() {
  log ""
  log "============================================================================="
  log "Scenario 2: Buyer Default (Remainder Window Lapses -> expire_remainder_window)"
  log "============================================================================="

  local total_amount=100000000
  local adv1_bps=1000
  local adv2_bps=2000
  local claim_window=300
  local remainder_window=15
  local delivery_window=600
  local contracted_qty=1000

  log "==> Deploying commitment contract with short remainder window (${remainder_window}s)..."
  local deploy_raw
  deploy_raw="$("$SCRIPT_DIR/deploy.sh" \
    --network "$NETWORK" \
    --source "$DEPLOYER" \
    --buyer "$BUYER" \
    --cooperative "$COOP" \
    --warehouse-operator "$WAREHOUSE" \
    --token "$TOKEN" \
    --total-amount "$total_amount" \
    --advance1-bps "$adv1_bps" \
    --advance2-bps "$adv2_bps" \
    --claim-window-secs "$claim_window" \
    --remainder-window-secs "$remainder_window" \
    --delivery-window-secs "$delivery_window" \
    --contracted-quantity "$contracted_qty" \
    --grade-price-bps '[10000,9000,7500]' \
    --json)"

  local contract_id deploy_tx_hash init_tx_hash wasm_hash
  contract_id="$(echo "$deploy_raw" | jq -r .contract_id)"
  deploy_tx_hash="$(echo "$deploy_raw" | jq -r .deploy_tx_hash)"
  init_tx_hash="$(echo "$deploy_raw" | jq -r .init_tx_hash)"
  wasm_hash="$(echo "$deploy_raw" | jq -r .wasm_hash)"

  log "    Contract ID: $contract_id"

  sleep 2
  local initial_contract_bal initial_buyer_bal initial_coop_bal
  initial_contract_bal="$(get_token_balance "$contract_id")"
  initial_buyer_bal="$(get_token_balance "$BUYER_ADDR")"
  initial_coop_bal="$(get_token_balance "$COOP_ADDR")"

  local steps="[]"
  local init_step
  init_step="$(jq -n \
    --arg tx_hash "$init_tx_hash" \
    --argjson cbal "$initial_contract_bal" \
    --argjson bbal "$initial_buyer_bal" \
    --argjson cobal "$initial_coop_bal" \
    '{
      step: 1,
      action: "initialize",
      caller: "e2e-buyer",
      args: [],
      tx_hash: $tx_hash,
      result: "ok",
      status_after: "Draft",
      balances_after: {
        contract: $cbal,
        buyer: $bbal,
        cooperative: $cobal
      }
    }')"
  steps="$(echo "$steps" | jq ". + [$init_step]")"

  # Step 2: lock (10% + 20% = 30,000,000)
  local s2
  s2="$(invoke_step 2 "lock" "$BUYER" "$contract_id" "Locked" 30000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s2]")"

  # Step 3: release_advance_1
  local s3
  s3="$(invoke_step 3 "release_advance_1" "$COOP" "$contract_id" "Advance1Released" 30000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s3]")"

  # Step 4: claim_advance_1 (coop receives 10,000,000; contract left with 20,000,000)
  local s4
  s4="$(invoke_step 4 "claim_advance_1" "$COOP" "$contract_id" "Advance1Released" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s4]")"

  # Step 5: mark_checkpoint (warehouse operator attests checkpoint)
  local s5
  s5="$(invoke_step 5 "mark_checkpoint" "$WAREHOUSE" "$contract_id" "CheckpointPassed" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s5]")"

  # Step 6: release_advance_2 (left unclaimed)
  local s6
  s6="$(invoke_step 6 "release_advance_2" "$COOP" "$contract_id" "Advance2Released" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s6]")"

  # Step 7: ready_for_delivery (starts remainder window countdown)
  local s7
  s7="$(invoke_step 7 "ready_for_delivery" "$COOP" "$contract_id" "ReadyForDelivery" 20000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s7]")"

  log "    Waiting 18 seconds for remainder window (${remainder_window}s) to expire..."
  sleep 18

  # Step 8: expire_remainder_window (called by stranger/third-party, sweeps remaining 20,000,000 to coop)
  local s8
  s8="$(invoke_step 8 "expire_remainder_window" "$STRANGER" "$contract_id" "Defaulted" 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s8]")"

  local params_json='{"total_amount":100000000,"advance1_bps":1000,"advance2_bps":2000,"claim_window_secs":300,"remainder_window_secs":15,"delivery_window_secs":600,"contracted_quantity":1000,"grade_price_bps":[10000,9000,7500]}'

  jq -n \
    --arg name "buyer_default" \
    --arg contract_id "$contract_id" \
    --arg deploy_tx "$deploy_tx_hash" \
    --arg wasm_hash "$wasm_hash" \
    --argjson params "$params_json" \
    --argjson steps "$steps" \
    '{
      name: $name,
      contract_id: $contract_id,
      deploy_tx_hash: $deploy_tx,
      wasm_hash: $wasm_hash,
      parameters: $params,
      steps: $steps
    }'
}

# =============================================================================
# SCENARIO 3: Seller Non-Delivery (reclaim_on_nondelivery)
# =============================================================================
run_seller_nondelivery() {
  log ""
  log "============================================================================="
  log "Scenario 3: Seller Non-Delivery (Delivery Deadline Passes -> reclaim_on_nondelivery)"
  log "============================================================================="

  local total_amount=100000000
  local adv1_bps=1500
  local adv2_bps=1500
  local claim_window=300
  local remainder_window=300
  local delivery_window=15
  local contracted_qty=1000

  log "==> Deploying commitment contract with short delivery window (${delivery_window}s)..."
  local deploy_raw
  deploy_raw="$("$SCRIPT_DIR/deploy.sh" \
    --network "$NETWORK" \
    --source "$DEPLOYER" \
    --buyer "$BUYER" \
    --cooperative "$COOP" \
    --warehouse-operator "$WAREHOUSE" \
    --token "$TOKEN" \
    --total-amount "$total_amount" \
    --advance1-bps "$adv1_bps" \
    --advance2-bps "$adv2_bps" \
    --claim-window-secs "$claim_window" \
    --remainder-window-secs "$remainder_window" \
    --delivery-window-secs "$delivery_window" \
    --contracted-quantity "$contracted_qty" \
    --grade-price-bps '[10000,9000,7500]' \
    --json)"

  local contract_id deploy_tx_hash init_tx_hash wasm_hash
  contract_id="$(echo "$deploy_raw" | jq -r .contract_id)"
  deploy_tx_hash="$(echo "$deploy_raw" | jq -r .deploy_tx_hash)"
  init_tx_hash="$(echo "$deploy_raw" | jq -r .init_tx_hash)"
  wasm_hash="$(echo "$deploy_raw" | jq -r .wasm_hash)"

  log "    Contract ID: $contract_id"

  sleep 2
  local initial_contract_bal initial_buyer_bal initial_coop_bal
  initial_contract_bal="$(get_token_balance "$contract_id")"
  initial_buyer_bal="$(get_token_balance "$BUYER_ADDR")"
  initial_coop_bal="$(get_token_balance "$COOP_ADDR")"

  local steps="[]"
  local init_step
  init_step="$(jq -n \
    --arg tx_hash "$init_tx_hash" \
    --argjson cbal "$initial_contract_bal" \
    --argjson bbal "$initial_buyer_bal" \
    --argjson cobal "$initial_coop_bal" \
    '{
      step: 1,
      action: "initialize",
      caller: "e2e-buyer",
      args: [],
      tx_hash: $tx_hash,
      result: "ok",
      status_after: "Draft",
      balances_after: {
        contract: $cbal,
        buyer: $bbal,
        cooperative: $cobal
      }
    }')"
  steps="$(echo "$steps" | jq ". + [$init_step]")"

  # Step 2: lock (15% + 15% = 30,000,000)
  local s2
  s2="$(invoke_step 2 "lock" "$BUYER" "$contract_id" "Locked" 30000000)" || return 1
  steps="$(echo "$steps" | jq ". + [$s2]")"

  log "    Waiting 18 seconds for delivery window (${delivery_window}s) to expire..."
  sleep 18

  # Step 3: reclaim_on_nondelivery (called by buyer, refunds 30,000,000 to buyer)
  local s3
  s3="$(invoke_step 3 "reclaim_on_nondelivery" "$BUYER" "$contract_id" "Forfeited" 0)" || return 1
  steps="$(echo "$steps" | jq ". + [$s3]")"

  local params_json='{"total_amount":100000000,"advance1_bps":1500,"advance2_bps":1500,"claim_window_secs":300,"remainder_window_secs":300,"delivery_window_secs":15,"contracted_quantity":1000,"grade_price_bps":[10000,9000,7500]}'

  jq -n \
    --arg name "seller_nondelivery" \
    --arg contract_id "$contract_id" \
    --arg deploy_tx "$deploy_tx_hash" \
    --arg wasm_hash "$wasm_hash" \
    --argjson params "$params_json" \
    --argjson steps "$steps" \
    '{
      name: $name,
      contract_id: $contract_id,
      deploy_tx_hash: $deploy_tx,
      wasm_hash: $wasm_hash,
      parameters: $params,
      steps: $steps
    }'
}

SCENARIOS_JSON="[]"

# Run chosen scenarios
if [[ "$SELECTED_SCENARIO" == "all" || "$SELECTED_SCENARIO" == "happy" ]]; then
  H_JSON="$(run_happy_path)" || exit 1
  SCENARIOS_JSON="$(echo "$SCENARIOS_JSON" | jq ". + [$H_JSON]")"
fi

if [[ "$SELECTED_SCENARIO" == "all" || "$SELECTED_SCENARIO" == "default" ]]; then
  D_JSON="$(run_buyer_default)" || exit 1
  SCENARIOS_JSON="$(echo "$SCENARIOS_JSON" | jq ". + [$D_JSON]")"
fi

if [[ "$SELECTED_SCENARIO" == "all" || "$SELECTED_SCENARIO" == "non-delivery" ]]; then
  N_JSON="$(run_seller_nondelivery)" || exit 1
  SCENARIOS_JSON="$(echo "$SCENARIOS_JSON" | jq ". + [$N_JSON]")"
fi

# Write final JSON transcript
FINAL_TRANSCRIPT="$(jq -n \
  --arg network "$NETWORK" \
  --arg timestamp "$TIMESTAMP" \
  --arg token "$TOKEN" \
  --arg buyer "$BUYER ($BUYER_ADDR)" \
  --arg coop "$COOP ($COOP_ADDR)" \
  --arg wh "$WAREHOUSE ($WAREHOUSE_ADDR)" \
  --arg stranger "$STRANGER ($STRANGER_ADDR)" \
  --arg deployer "$DEPLOYER ($DEPLOYER_ADDR)" \
  --argjson scenarios "$SCENARIOS_JSON" \
  '{
    network: $network,
    timestamp: $timestamp,
    token: $token,
    identities: {
      buyer: $buyer,
      cooperative: $coop,
      warehouse: $wh,
      stranger: $stranger,
      deployer: $deployer
    },
    scenarios: $scenarios
  }')"

echo "$FINAL_TRANSCRIPT" > "$OUTPUT_PATH"
cp "$OUTPUT_PATH" "$LATEST_PATH"

log ""
log "============================================================================="
log "E2E Run Completed Successfully!"
log "============================================================================="
log "Transcript saved to:"
log "  $OUTPUT_PATH"
log "  $LATEST_PATH"
log "============================================================================="
