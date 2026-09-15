#!/usr/bin/env bash
# Create the testnet accounts the examples use, and fund them.
#
# - nineveh-publisher publishes the contracts, then plays in them.
# - nineveh-player is a second person, so the market has someone to sell to.
#
# Keys are kept in .aptos/config.yaml, here, which git ignores.
set -euo pipefail
cd "$(dirname "$0")"

for profile in nineveh-publisher nineveh-player; do
  # A missing profile shows as an empty result, not an error.
  if aptos config show-profiles --profile "$profile" 2>/dev/null | grep -q '"account"'; then
    echo "$profile exists"
  else
    # `aptos init` also asks the faucet for coins; testnet's may send you to its web page.
    # No input: it generates a new key instead of asking for one.
    aptos init --profile "$profile" --network testnet --assume-yes --skip-faucet </dev/null >/dev/null
    echo "created $profile"
  fi
done

echo
for profile in nineveh-publisher nineveh-player; do
  address=$(aptos config show-profiles --profile "$profile" | grep -Eo '"account": "[0-9a-fx]+"' | grep -Eo '[0-9a-f]{64}')
  balance=$(aptos account balance --profile "$profile" 2>/dev/null | grep -Eo '"balance": [0-9]+' | grep -Eo '[0-9]+' || echo 0)
  echo "$profile: 0x$address  balance: ${balance:-0} octas"
  if [[ "${balance:-0}" == "0" ]]; then
    aptos account fund-with-faucet --profile "$profile" >/dev/null 2>&1 \
      && echo "    funded from the faucet" \
      || echo "    fund it at https://aptos.dev/network/faucet?address=0x$address"
  fi
done
