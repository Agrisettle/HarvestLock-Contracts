#!/usr/bin/env bash
# Issue a classic asset, deploy its SAC, set up trustlines, and mint to the buyer.
#
# Issue #284: Every live walkthrough used the native XLM SAC, which hides
# the trustline requirement: a cooperative without a trustline can't receive
# claim_advance_* or settle payouts. A pilot settles in an issued asset,
# so live tests should be able to as well.
#
# What this script does:
#   1. Resolves / creates and funds the issuer identity on the target network.
#   2. Optionally sets AUTH_REVOCABLE and/or AUTH_CLAWBACK_ENABLED flags on the issuer.
#   3. Derives / deploys the Soroban Asset Contract (SAC) for the classic asset (stellar contract asset deploy).
#   4. Ensures buyer and cooperative identities exist and adds trustlines for both.
#   5. Mints the asset to the buyer via payment from the issuer.
#   6. Prints the SAC contract id for use as initialize's `--token`.
#
# See docs/trustline-prerequisites.md for the per-role trustline prerequisites.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/test-asset.sh [OPTIONS]

Issue a classic asset on Stellar, deploy its Soroban Asset Contract (SAC),
add trustlines for the buyer and cooperative, and mint initial tokens to the buyer.

Options:
  -n, --network <NAME>        Stellar network to use (default: testnet, or $STELLAR_NETWORK)
  -a, --asset-code <CODE>     Asset code to issue (default: HLUSD, or $ASSET_CODE)
  -i, --issuer <IDENTITY>     Issuer identity name/alias (default: test-issuer, or $ISSUER)
  -b, --buyer <IDENTITY>      Buyer identity name/alias (default: buyer, or $BUYER)
  -c, --cooperative <IDENTITY> Cooperative identity name/alias (default: cooperative, or $COOPERATIVE)
  -m, --amount <STROOPS>      Amount in stroops (7 decimals) to mint to buyer (default: 100000000 = 10 units)
      --auth-revocable        Set AUTH_REVOCABLE flag on issuer account
      --auth-clawback         Set AUTH_CLAWBACK_ENABLED flag on issuer account
      --all-flags             Set both AUTH_REVOCABLE and AUTH_CLAWBACK_ENABLED on issuer account
  -q, --quiet                 Print only the SAC contract ID to stdout
  -h, --help                  Show this help message

Environment:
  STELLAR_NETWORK             Default network if --network is omitted (default: testnet)
  ASSET_CODE                  Default asset code if --asset-code is omitted (default: HLUSD)
  ISSUER                      Default issuer identity if --issuer is omitted (default: test-issuer)
  BUYER                       Default buyer identity if --buyer is omitted (default: buyer)
  COOPERATIVE                 Default cooperative identity if --cooperative is omitted (default: cooperative)
  MINT_AMOUNT                 Default mint amount if --amount is omitted (default: 100000000)

Examples:
  # Standard setup on testnet with default HLUSD:
  scripts/test-asset.sh

  # Capture SAC ID in a variable for initialize:
  TOKEN=$(scripts/test-asset.sh --quiet)

  # Custom asset code with issuer flags enabled:
  scripts/test-asset.sh --asset-code USDC --all-flags --amount 500000000
USAGE
}

NETWORK="${STELLAR_NETWORK:-testnet}"
ASSET_CODE="${ASSET_CODE:-HLUSD}"
ISSUER="${ISSUER:-test-issuer}"
BUYER="${BUYER:-buyer}"
COOP="${COOPERATIVE:-cooperative}"
MINT_AMOUNT="${MINT_AMOUNT:-100000000}"
SET_REVOCABLE=0
SET_CLAWBACK=0
QUIET=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -n|--network)
      NETWORK="$2"
      shift 2
      ;;
    -a|--asset-code)
      ASSET_CODE="$2"
      shift 2
      ;;
    -i|--issuer)
      ISSUER="$2"
      shift 2
      ;;
    -b|--buyer)
      BUYER="$2"
      shift 2
      ;;
    -c|--cooperative)
      COOP="$2"
      shift 2
      ;;
    -m|--amount)
      MINT_AMOUNT="$2"
      shift 2
      ;;
    --auth-revocable)
      SET_REVOCABLE=1
      shift
      ;;
    --auth-clawback)
      SET_CLAWBACK=1
      shift
      ;;
    --all-flags|--issuer-flags)
      SET_REVOCABLE=1
      SET_CLAWBACK=1
      shift
      ;;
    -q|--quiet)
      QUIET=1
      shift
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

log() {
  if [[ "$QUIET" -eq 0 ]]; then
    echo "$@" >&2
  fi
}

run_tx() {
  local out=""
  for attempt in {1..3}; do
    if out="$("$@" 2>&1)"; then
      return 0
    fi
    sleep 3
  done
  echo "$out" >&2
  return 1
}

if ! command -v stellar >/dev/null 2>&1; then
  cat >&2 <<'EOF'
error: 'stellar' CLI not found on PATH.

Install stellar-cli (e.g. v30.0.1) as documented in docs/dev-setup-troubleshooting.md.
EOF
  exit 1
fi

# Ensure an identity exists and has testnet funds
ensure_identity() {
  local identity="$1"
  if ! stellar keys address "$identity" >/dev/null 2>&1; then
    log "==> Generating and funding identity '$identity' on network '$NETWORK'..."
    stellar keys generate "$identity" --network "$NETWORK" --fund >/dev/null 2>&1
  else
    local addr
    addr="$(stellar keys address "$identity")"
    log "==> Identity '$identity' exists: $addr"
    # Ensure funded on testnet if friendbot is available
    stellar keys fund "$identity" --network "$NETWORK" >/dev/null 2>&1 || true
  fi
}

log "==> Preparing identities for network '$NETWORK'..."
ensure_identity "$ISSUER"
ensure_identity "$BUYER"
ensure_identity "$COOP"

ISSUER_ADDR="$(stellar keys address "$ISSUER")"
BUYER_ADDR="$(stellar keys address "$BUYER")"
COOP_ADDR="$(stellar keys address "$COOP")"

ASSET_ID="${ASSET_CODE}:${ISSUER_ADDR}"
log "==> Target classic asset: $ASSET_ID"

# Optionally set issuer flags (AUTH_REVOCABLE / AUTH_CLAWBACK_ENABLED)
if [[ "$SET_REVOCABLE" -eq 1 || "$SET_CLAWBACK" -eq 1 ]]; then
  FLAGS=()
  if [[ "$SET_REVOCABLE" -eq 1 ]]; then
    FLAGS+=(--set-revocable)
  fi
  if [[ "$SET_CLAWBACK" -eq 1 ]]; then
    FLAGS+=(--set-clawback-enabled)
  fi
  log "==> Setting flags on issuer '$ISSUER' (${FLAGS[*]} entropy)..."
  run_tx stellar tx new set-options --source-account "$ISSUER" --network "$NETWORK" "${FLAGS[@]}"
fi

# Derive or deploy the Soroban Asset Contract (SAC)
log "==> Deploying Soroban Asset Contract (SAC) for '$ASSET_ID'..."
SAC_ID="$(stellar contract id asset --asset "$ASSET_ID" --network "$NETWORK" 2>/dev/null || true)"

DEPLOY_OUT="$(stellar contract asset deploy --asset "$ASSET_ID" --source-account "$ISSUER" --network "$NETWORK" 2>&1 || true)"
if echo "$DEPLOY_OUT" | grep -q "ExistingValue"; then
  log "    SAC already deployed on-chain for $ASSET_ID ($SAC_ID)"
elif echo "$DEPLOY_OUT" | grep -E -q '^C[A-Z0-9]{55}'; then
  SAC_ID="$(echo "$DEPLOY_OUT" | grep -E '^C[A-Z0-9]{55}' | tail -n1)"
fi

if [[ -z "$SAC_ID" ]]; then
  echo "error: failed to deploy or resolve SAC contract ID for $ASSET_ID" >&2
  echo "$DEPLOY_OUT" >&2
  exit 1
fi
log "==> SAC Contract ID: $SAC_ID"

# Add trustlines for buyer and cooperative
log "==> Setting up trustline for buyer ($BUYER -> $ASSET_ID)..."
run_tx stellar tx new change-trust --source-account "$BUYER" --line "$ASSET_ID" --network "$NETWORK"

log "==> Setting up trustline for cooperative ($COOP -> $ASSET_ID)..."
run_tx stellar tx new change-trust --source-account "$COOP" --line "$ASSET_ID" --network "$NETWORK"

# Mint initial tokens to buyer
log "==> Minting $MINT_AMOUNT stroops of $ASSET_CODE to buyer ($BUYER)..."
run_tx stellar tx new payment --source-account "$ISSUER" --destination "$BUYER_ADDR" --asset "$ASSET_ID" --amount "$MINT_AMOUNT" --network "$NETWORK"

if [[ "$QUIET" -eq 1 ]]; then
  echo "$SAC_ID"
else
  cat <<EOF

=============================================================================
Test Asset & SAC Deployment Summary
=============================================================================
Network:              $NETWORK
Asset ID:             $ASSET_ID
Asset Code:           $ASSET_CODE
Decimals:             7 (Stellar classic standard)
Issuer:               $ISSUER ($ISSUER_ADDR)
Buyer:                $BUYER ($BUYER_ADDR)
Cooperative:          $COOP ($COOP_ADDR)
Buyer Trustline:      Configured
Coop Trustline:       Configured
Minted to Buyer:      $MINT_AMOUNT stroops ($(( MINT_AMOUNT / 10000000 )).$(( MINT_AMOUNT % 10000000 )) $ASSET_CODE)

SAC Contract ID (token for initialize):
$SAC_ID
EOF
fi
