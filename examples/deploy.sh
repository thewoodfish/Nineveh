#!/usr/bin/env bash
# Publish the examples, each at an object address of its own, and write the addresses
# to deployed.<network>.env. One funded account publishes them all.
#
#   ./setup.sh && ./deploy.sh              # all four
#   ./deploy.sh 03-market                  # just the one you need
#   NETWORK=devnet ./setup.sh && NETWORK=devnet ./deploy.sh 03-market
set -euo pipefail
cd "$(dirname "$0")"

ALL="01-counter:counter:COUNTER 02-guestbook:guestbook:GUESTBOOK 03-market:market:MARKET \
04-arena:arena:ARENA"

# Named on the command line, or all of them. A name that isn't an example is a typo
# worth stopping for: carrying on would publish the wrong set. Checked before anything
# on disk is touched, so a typo leaves no half-made deployed.<network>.env behind.
if [[ $# -eq 0 ]]; then
  wanted=$ALL
else
  wanted=""
  for want in "$@"; do
    found=""
    for example in $ALL; do
      [[ ${example%%:*} == "$want" ]] && found=$example
    done
    if [[ -z $found ]]; then
      echo "no example called \`$want\`" >&2
      printf 'one of:' >&2
      for example in $ALL; do printf ' %s' "${example%%:*}" >&2; done
      echo >&2
      exit 1
    fi
    wanted="$wanted $found"
  done
fi

NETWORK=${NETWORK:-testnet}
profile="nineveh-publisher-$NETWORK"
deployed="deployed.$NETWORK.env"
touch "$deployed"

# Publishing again would make another object at another address, so anything already
# in deployed.<network>.env is left alone. Delete its line to publish it afresh.
for example in $wanted; do
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
echo "Then ./play.sh keeps whatever you published busy."
