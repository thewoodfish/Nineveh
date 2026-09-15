#!/usr/bin/env bash
# Create the accounts the examples use on a network, and fund them.
#
# - the publisher publishes the contracts, then plays in them.
# - the player is a second person, so the market has someone to sell to.
#
#   ./setup.sh              # testnet (default)
#   NETWORK=devnet ./setup.sh
#
# Keys are kept in .aptos/config.yaml, here, which git ignores. Devnet's faucet funds
# accounts over its API; testnet's only through its web page, so it prints the link.
set -euo pipefail
cd "$(dirname "$0")"

NETWORK=${NETWORK:-testnet}

for role in publisher player; do
  profile="nineveh-$role-$NETWORK"
  # A missing profile shows as an empty result, not an error.
  if aptos config show-profiles --profile "$profile" 2>/dev/null | grep -q '"account"'; then
    echo "$profile exists"
  else
    # No input: it generates a new key instead of asking for one.
    aptos init --profile "$profile" --network "$NETWORK" --assume-yes --skip-faucet \
      </dev/null >/dev/null
    echo "created $profile"
  fi
done

echo
for role in publisher player; do
  profile="nineveh-$role-$NETWORK"
  address=0x$(aptos config show-profiles --profile "$profile" \
    | grep -Eo '"account": "[0-9a-f]+"' | grep -Eo '[0-9a-f]{64}')
  balance=$(aptos account balance --profile "$profile" 2>/dev/null \
    | grep -Eo '"balance": [0-9]+' | grep -Eo '[0-9]+' | head -1 || true)
  if [[ "${balance:-0}" -lt 10000000 ]]; then
    if [[ "$NETWORK" == "devnet" ]]; then
      curl -fsS -X POST "https://faucet.devnet.aptoslabs.com/mint?amount=100000000&address=$address" \
        >/dev/null && echo "$profile: $address  funded"
    else
      echo "$profile: $address"
      echo "    fund it at https://aptos.dev/network/faucet?address=$address"
    fi
  else
    echo "$profile: $address  balance: $balance octas"
  fi
done
