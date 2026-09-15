#!/usr/bin/env bash
# Publish every example to testnet, each at an object address of its own, and write
# the addresses to deployed.env. One funded account publishes them all.
#
#   ./setup.sh     # once: the accounts, and how to fund them
#   ./deploy.sh
set -euo pipefail
cd "$(dirname "$0")"

profile=nineveh-publisher
: > deployed.env.tmp
for example in 01-counter:counter:COUNTER 02-guestbook:guestbook:GUESTBOOK 03-market:market:MARKET 04-arena:arena:ARENA; do
  IFS=: read -r dir name var <<<"$example"
  echo "==> $dir"
  output=$(aptos move deploy-object \
    --package-dir "$dir" \
    --address-name "$name" \
    --profile "$profile" \
    --assume-yes 2>&1) || { echo "$output"; exit 1; }
  address=$(grep -Eo 'object address 0x[0-9a-f]+' <<<"$output" | grep -Eo '0x[0-9a-f]+' | tail -1)
  if [[ -z "$address" ]]; then
    echo "$output"
    echo "couldn't find the object address in the output above" >&2
    exit 1
  fi
  echo "    $name is at $address"
  echo "$var=$address" >> deployed.env.tmp
done
mv deployed.env.tmp deployed.env
echo
echo "Addresses are in deployed.env. Paste one into Studio's New project."
