#!/bin/sh
# scripts/bootstrap-identities.sh — create and fund the four HarvestLock test
# role identities on a network.
#
# Usage:
#   ./scripts/bootstrap-identities.sh --network testnet|futurenet|local
#
# Configures the network entry for the target network if it is missing, then
# creates (if missing) and funds via friendbot the deployer, buyer,
# cooperative and warehouse identities. Prints each role's address.
#
# Idempotent: an identity that already present is not regenerated, so running
# the script twice on a clean machine leaves four funded identities and exits
# zero both times.
#
# Requires:
#   - `stellar` CLI on PATH (stellar-cli v28+, prebuilt per
#     docs/dev-setup-troubleshooting.md)
#   - network access to the target network's friendbot (testnet/futurenet are
#     funded by Stellar's public friendbot; `local` needs a local node with
#     friendbot running on http://localhost:8000)
set -eu

usage() {
    echo "Usage: $0 --network testnet|futurenet|local" >&2
    exit 2
}

err() {
    echo "error: $*" >&2
    exit 2
}

network=""

while [ $# -gt 0 ]; do
    case "$1" in
        --network)
            [ $# -gt 1 ] || err "--network requires a value"
            network="$2"; shift 2 ;;
        --network=*)
            network="${1#--network=}"; shift ;;
        --help|-h) usage ;;
        --*) err "unknown option: $1" ;;
        *) err "unexpected argument: $1" ;;
    esac
done

[ -n "$network" ] || err "--network is required"

# Network entry (alias, RPC URL, passphrase) for each supported network.
case "$network" in
    testnet)
        network_alias="testnet"
        rpc_url="https://soroban-testnet.stellar.org"
        passphrase="Testnet SDF Network ; September 2015"
        ;;
    futurenet)
        network_alias="futurenet"
        rpc_url="https://rpc-futurenet.stellar.org"
        passphrase="Testnet SDF Network ; September 2015"
        ;;
    local)
        network_alias="local"
        rpc_url="http://localhost:8000"
        passphrase="Standalone Network ; February 2017"
        ;;
    *) err "--network must be one of testnet, futurenet or local (got: $network)" ;;
esac

command -v stellar >/dev/null 2>&1 || err "'stellar' CLI not found on PATH (install stellar-cli, see docs/dev-setup-troubleshooting.md)"

# 1. Configure the network entry if the keystore does not know it yet. The
#    alias's first whitespace-delimited field is compared exactly.
if ! stellar network list 2>/dev/null | awk '{print $1}' | grep -qxF "$network_alias"; then
    echo "==> Configuring network entry: ${network_alias} (${rpc_url})" >&2
    stellar network add "$network_alias" --rpc-url "$rpc_url" --network-passphrase "$passphrase" >/dev/null
else
    echo "==> Network entry already configured: ${network_alias}" >&2
fi

# 2. Create (if missing) and fund each role identity, then print its address.
#    `stellar keys address` succeeds for an existing identity, so a second run
#    skips generation entirely and exits zero.
for role in deployer buyer cooperative warehouse; do
    if ! stellar keys address "$role" >/dev/null 2>&1; then
        echo "==> Creating and funding identity: $role (${network})" >&2
        stellar keys generate "$role" --network "$network_alias" --fund
    else
        echo "==> Identity already present: $role" >&2
    fi
    printf '%s: %s\n' "$role" "$(stellar keys address "$role")"
done

echo "==> All four identities are ready on ${network}: deployer, buyer, cooperative, warehouse" >&2
