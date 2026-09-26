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

# Whether any module still lives at an address. Devnet is wiped every week or so, and
# what it leaves behind is an address that still resolves and answers with an empty
# module list, so "the file names an address" is not the same question as "the contract
# is there". Trusting the file is how you end up pasting a dead address into Studio.
published() { # published ADDRESS
  local body
  body=$(curl -fsS --max-time 15 \
    ${NODE_API_KEY:+-H "Authorization: Bearer $NODE_API_KEY"} \
    "https://fullnode.$NETWORK.aptoslabs.com/v1/accounts/$1/modules" 2>/dev/null) || return 1
  [[ -n $body && $body != "[]" ]]
}

# Publishing again would make another object at another address, so anything already in
# deployed.<network>.env and still on chain is left alone. Delete its line to publish it
# afresh anyway.
for example in $wanted; do
  IFS=: read -r dir name var <<<"$example"
  if grep -q "^$var=" "$deployed"; then
    at=$(grep "^$var=" "$deployed" | cut -d= -f2)
    if published "$at"; then
      echo "==> $dir (already at $at)"
      continue
    fi
    echo "==> $dir (nothing is left at $at, publishing it again)"
    { grep -v "^$var=" "$deployed" || true; } > "$deployed.tmp"
    mv "$deployed.tmp" "$deployed"
  else
    echo "==> $dir"
  fi
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
