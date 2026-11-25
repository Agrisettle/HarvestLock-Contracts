#!/bin/sh
# scripts/deploy.sh — scripted deploy for the HarvestLock escrow contract.
#
# Usage:
#   ./scripts/deploy.sh --network testnet|futurenet|local [--source <account>] <terms.json>
#   ./scripts/deploy.sh --network testnet [options...] (see --help)
#
# Builds the WASM artifact, uploads it to the target network only when its
# hash is not already installed there (re-running with the same WASM skips
# the upload), deploys a contract instance, and calls `initialize` with the
# terms as the `buyer` (or `--source`) identity. Prints the deployed contract ID
# and WASM hash as JSON on stdout.
#
# Requires:
#   - `stellar` CLI on PATH (stellar-cli v28+, prebuilt per
#     docs/dev-setup-troubleshooting.md)
#   - `jq` on PATH (to read the terms JSON)
#   - a funded source identity in the stellar keystore
#     (see scripts/bootstrap-identities.sh)
#
# The terms JSON must name every `initialize` argument:
#   buyer, cooperative, warehouse_operator, token,
#   total_amount, advance1_bps, advance2_bps,
#   claim_window_secs, remainder_window_secs, delivery_window_secs,
#   contracted_quantity, grade_price_bps, oracle_config
# `oracle_config` may be `null` for a plain deal, or an object with
#   oracle_contract, price_asset, max_age_secs, denominated_amount
# for an oracle-configured deal (passed via --oracle_config-file-path).
#
# Since #270 the eight numeric/list terms above are sent to `initialize` as
# one named `CommitmentTerms` JSON object (`--terms`), not as positional
# flags; the parties, the token and `oracle_config` stay separate arguments.
set -eu

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
CRATE_DIR="$ROOT_DIR/contracts/escrow"
WASM="$CRATE_DIR/target/wasm32v1-none/release/harvestlock_escrow.wasm"

usage() {
    echo "Usage: $0 --network testnet|futurenet|local [--source <account>] <terms.json>" >&2
    exit 2
}

err() {
    echo "error: $*" >&2
    exit 2
}

network=""
source_account="buyer"
terms_file=""

buyer_addr=""
coop_addr=""
wh_addr=""
token_addr=""
total_amount=""
advance1_bps=""
advance2_bps=""
claim_window_secs=""
remainder_window_secs=""
delivery_window_secs=""
contracted_quantity=""
grade_price_bps=""
oracle_config="null"

while [ $# -gt 0 ]; do
    case "$1" in
        --network)
            [ $# -gt 1 ] || err "--network requires a value"
            network="$2"; shift 2 ;;
        --network=*)
            network="${1#--network=}"; shift ;;
        --source|--source-account)
            [ $# -gt 1 ] || err "$1 requires a value"
            source_account="$2"; shift 2 ;;
        --source=*|--source-account=*)
            source_account="${1#*=}"; shift ;;
        --buyer)
            [ $# -gt 1 ] || err "$1 requires a value"
            buyer_addr="$2"; shift 2 ;;
        --cooperative)
            [ $# -gt 1 ] || err "$1 requires a value"
            coop_addr="$2"; shift 2 ;;
        --warehouse-operator)
            [ $# -gt 1 ] || err "$1 requires a value"
            wh_addr="$2"; shift 2 ;;
        --token)
            [ $# -gt 1 ] || err "$1 requires a value"
            token_addr="$2"; shift 2 ;;
        --total-amount)
            [ $# -gt 1 ] || err "$1 requires a value"
            total_amount="$2"; shift 2 ;;
        --advance1-bps)
            [ $# -gt 1 ] || err "$1 requires a value"
            advance1_bps="$2"; shift 2 ;;
        --advance2-bps)
            [ $# -gt 1 ] || err "$1 requires a value"
            advance2_bps="$2"; shift 2 ;;
        --claim-window-secs)
            [ $# -gt 1 ] || err "$1 requires a value"
            claim_window_secs="$2"; shift 2 ;;
        --remainder-window-secs)
            [ $# -gt 1 ] || err "$1 requires a value"
            remainder_window_secs="$2"; shift 2 ;;
        --delivery-window-secs)
            [ $# -gt 1 ] || err "$1 requires a value"
            delivery_window_secs="$2"; shift 2 ;;
        --contracted-quantity)
            [ $# -gt 1 ] || err "$1 requires a value"
            contracted_quantity="$2"; shift 2 ;;
        --grade-price-bps)
            [ $# -gt 1 ] || err "$1 requires a value"
            grade_price_bps="$2"; shift 2 ;;
        --oracle-config)
            [ $# -gt 1 ] || err "$1 requires a value"
            oracle_config="$2"; shift 2 ;;
        --json)
            shift ;; # accepted for backwards-compatibility
        --help|-h) usage ;;
        --*) err "unknown option: $1" ;;
        *)
            if [ -z "$terms_file" ]; then
                terms_file="$1"; shift
            else
                err "unexpected argument: $1"
            fi ;;
    esac
done

[ -n "$network" ] || err "--network is required"
case "$network" in
    testnet|futurenet|local) ;;
    *) err "--network must be one of testnet, futurenet or local (got: $network)" ;;
esac

command -v stellar >/dev/null 2>&1 || err "'stellar' CLI not found on PATH (install stellar-cli, see docs/dev-setup-troubleshooting.md)"
command -v jq >/dev/null 2>&1 || err "'jq' not found on PATH (install jq to parse the terms JSON)"
command -v sha256sum >/dev/null 2>&1 || err "'sha256sum' not found on PATH (needed to compute the WASM hash)"

# The source keystore identity signs both the deploy and the `initialize` call.
stellar keys address "$source_account" >/dev/null 2>&1 || err "$source_account identity not found in the stellar keystore (run scripts/bootstrap-identities.sh first)"

# 1. Build the WASM artifact (package lives in contracts/escrow).
echo "==> Building WASM artifact ($network)" >&2
( cd "$CRATE_DIR" && stellar contract build --locked )
[ -f "$WASM" ] || err "WASM artifact not produced at $WASM"

# 2. Compute the WASM hash, and skip the (transaction-charging) upload when
#    this hash is already installed on the target network. A deploy
#    simulation (`--send=no`) fails when the hash is unknown, so it doubles
#    as an installed-wasm probe. Stellar stores each WASM once keyed by its
#    hash, so re-running with the same WASM never stores a duplicate.
wasm_hash=$(sha256sum "$WASM" | awk '{print $1}')
echo "==> WASM hash: $wasm_hash" >&2

if stellar contract deploy --wasm-hash "$wasm_hash" --source-account "$source_account" --network "$network" --send=no >/dev/null 2>&1; then
    echo "==> WASM $wasm_hash already installed on $network — skipping upload" >&2
else
    echo "==> Uploading WASM to $network" >&2
    wasm_hash=$(stellar contract upload --wasm "$WASM" --source-account "$source_account" --network "$network" | tail -1)
    echo "==> WASM uploaded: $wasm_hash" >&2
fi

# 3. Read the terms from file if provided.
if [ -n "$terms_file" ]; then
    [ -f "$terms_file" ] || err "terms file not found: $terms_file"
    buyer_addr=$(jq -r '.buyer' "$terms_file")
    coop_addr=$(jq -r '.cooperative' "$terms_file")
    wh_addr=$(jq -r '.warehouse_operator' "$terms_file")
    token_addr=$(jq -r '.token' "$terms_file")
    total_amount=$(jq -r '.total_amount' "$terms_file")
    advance1_bps=$(jq -r '.advance1_bps' "$terms_file")
    advance2_bps=$(jq -r '.advance2_bps' "$terms_file")
    claim_window_secs=$(jq -r '.claim_window_secs' "$terms_file")
    remainder_window_secs=$(jq -r '.remainder_window_secs' "$terms_file")
    delivery_window_secs=$(jq -r '.delivery_window_secs' "$terms_file")
    contracted_quantity=$(jq -r '.contracted_quantity' "$terms_file")
    grade_price_bps=$(jq -c '.grade_price_bps' "$terms_file")
    oracle_config=$(jq -c '.oracle_config' "$terms_file")
fi

# If buyer was specified as a keystore name and stellar keys address knows it, resolve it.
if [ -n "$buyer_addr" ] && ! echo "$buyer_addr" | grep -qE '^G[A-Z0-9]{55}$'; then
    buyer_addr=$(stellar keys address "$buyer_addr" 2>/dev/null || echo "$buyer_addr")
fi
if [ -n "$coop_addr" ] && ! echo "$coop_addr" | grep -qE '^G[A-Z0-9]{55}$'; then
    coop_addr=$(stellar keys address "$coop_addr" 2>/dev/null || echo "$coop_addr")
fi
if [ -n "$wh_addr" ] && ! echo "$wh_addr" | grep -qE '^G[A-Z0-9]{55}$'; then
    wh_addr=$(stellar keys address "$wh_addr" 2>/dev/null || echo "$wh_addr")
fi

# Validate every required terms field is present and non-null.
require_field() {
    case "$2" in
        ""|null) err "terms parameter is missing or required: $1" ;;
    esac
}
require_field buyer "$buyer_addr"
require_field cooperative "$coop_addr"
require_field warehouse_operator "$wh_addr"
require_field token "$token_addr"
require_field total_amount "$total_amount"
require_field advance1_bps "$advance1_bps"
require_field advance2_bps "$advance2_bps"
require_field claim_window_secs "$claim_window_secs"
require_field remainder_window_secs "$remainder_window_secs"
require_field delivery_window_secs "$delivery_window_secs"
require_field contracted_quantity "$contracted_quantity"
require_field grade_price_bps "$grade_price_bps"

# The eight agreed terms as one struct (#270).
terms_json=$(jq -c -n \
    --argjson total_amount "$total_amount" \
    --argjson advance1_bps "$advance1_bps" \
    --argjson advance2_bps "$advance2_bps" \
    --argjson claim_window_secs "$claim_window_secs" \
    --argjson remainder_window_secs "$remainder_window_secs" \
    --argjson delivery_window_secs "$delivery_window_secs" \
    --argjson contracted_quantity "$contracted_quantity" \
    --argjson grade_price_bps "$grade_price_bps" \
    '{total_amount: $total_amount, advance1_bps: $advance1_bps, advance2_bps: $advance2_bps, claim_window_secs: $claim_window_secs, remainder_window_secs: $remainder_window_secs, delivery_window_secs: $delivery_window_secs, contracted_quantity: $contracted_quantity, grade_price_bps: $grade_price_bps}')

# 4. Deploy a contract instance from the installed WASM.
echo "==> Deploying contract instance" >&2
contract_id=$(stellar contract deploy --wasm-hash "$wasm_hash" --source-account "$source_account" --network "$network")
echo "==> Contract ID: $contract_id" >&2

# 5. Call `initialize` with the terms as the source identity.
echo "==> Calling initialize" >&2
if [ "$oracle_config" = "null" ] || [ -z "$oracle_config" ]; then
    stellar contract invoke --id "$contract_id" --source-account "$source_account" --network "$network" -- \
        initialize \
        --buyer "$buyer_addr" --cooperative "$coop_addr" --warehouse-operator "$wh_addr" \
        --token "$token_addr" \
        --terms "$terms_json" \
        --oracle-config null
else
    oracle_file=$(mktemp)
    printf '%s' "$oracle_config" > "$oracle_file"
    stellar contract invoke --id "$contract_id" --source-account "$source_account" --network "$network" -- \
        initialize \
        --buyer "$buyer_addr" --cooperative "$coop_addr" --warehouse-operator "$wh_addr" \
        --token "$token_addr" \
        --terms "$terms_json" \
        --oracle_config-file-path "$oracle_file"
    rm -f "$oracle_file"
fi

# 6. Print the deployed contract ID and WASM hash as JSON.
printf '{"contract_id":"%s","wasm_hash":"%s","network":"%s"}\n' "$contract_id" "$wasm_hash" "$network"
echo "==> Deployed $contract_id (WASM $wasm_hash) on $network" >&2
