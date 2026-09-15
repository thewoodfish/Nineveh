#!/usr/bin/env bash
# Publish every example, each at an object address of its own, and write the addresses
# to deployed.<network>.env. One funded account publishes them all.
#
#   ./setup.sh && ./deploy.sh
#   NETWORK=devnet ./setup.sh && NETWORK=devnet ./deploy.sh
set -euo pipefail
cd "$(dirname "$0")"

NETWORK=${NETWORK:-testnet}
profile="nineveh-publisher-$NETWORK"
deployed="deployed.$NETWORK.env"
touch "$deployed"

# Publishing again would make another object at another address, so anything already
# in deployed.<network>.env is left alone. Delete its line to publish it afresh.
for example in 01-counter:counter:COUNTER 02-guestbook:guestbook:GUESTBOOK 03-market:market:MARKET 04-arena:arena:ARENA; do
  IFS=: read -r dir name var <<<"$example"
  if grep -q "^$var=" "$deployed"; then
    echo "==> $dir (already at $(grep "^$var=" "$deployed" | cut -d= -f2))"
    continue
  fi
  echo "==> $dir"
  output=$(aptos move deploy-object \
    --package-dir "$dir" \
    --address-name "$name" \
    --profile "$profile" \
    --max-gas 200000 --gas-unit-price 100 \
    --assume-yes 2>&1) || { echo "$output"; exit 1; }
  address=$(grep -Eo 'object address 0x[0-9a-f]+' <<<"$output" | grep -Eo '0x[0-9a-f]+' | tail -1)
  if [[ -z "$address" ]]; then
    echo "$output"
    echo "couldn't find the object address in the output above" >&2
    exit 1
  fi
  echo "    $name is at $address"
  echo "$var=$address" >> "$deployed"
done
echo
echo "Addresses are in $deployed. Paste one into Studio's New project."
