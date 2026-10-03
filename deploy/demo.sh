#!/usr/bin/env bash
# Keep the market example alive on devnet, for the demo page to drive.
#
# The address is stable, and that is the whole point of this script.
#
# It used to republish every few hours, which gave a new object address every time,
# because `deploy-object` derives one from the publisher's GUID creation number. A
# project created against the previous address went on running perfectly and never saw
# another row — no error, no warning, a cursor advancing against a contract nobody was
# using any more. The reason for republishing was that a free project must start within
# six hours of the chain's tip, so an older contract could not be followed with "All of
# its history".
#
# It doesn't need to be. A `table:` source learns its handle from any write to the
# parent resource, not only the write that created it (ADR 0012), and this contract
# writes `Market` on every list, buy and cancel. So a project started "From now on" has
# working `listings` and `credits` tables after a single transaction, and the address
# can stay put forever.
#
# What is left is a health check: the contract only moves when devnet is wiped, which is
# about weekly. Most runs send no transaction at all.
#
#   NINEVEH_DEMO_DIR=/var/lib/nineveh/public ./demo.sh
#
# To force a fresh publish — after changing the Move source, say — delete `demo.json`
# and run it again.
#
# A failure leaves the previous deployment and the previous `demo.json` untouched, so
# the page keeps working on the last good one.
#
# NODE_API_KEY is not optional here, however much it is for a visitor. Publishing a
# package is expensive in the node's terms — fetch the account, simulate four and a half
# kilobytes of bytecode, submit, poll — and two publishes are enough to exhaust the
# anonymous per-IP allowance of 40,000 compute units per five minutes. That allowance is
# also shared with everything else leaving this machine. The key is Nineveh's own, on
# Nineveh's own server; nobody visiting the page ever needs one.
set -euo pipefail

if [[ -z ${NODE_API_KEY:-} ]]; then
  echo "demo: NODE_API_KEY is unset — publishing will hit the anonymous rate limit" >&2
  echo "      and leave the demo contract stale. Get a devnet key at https://geomi.dev" >&2
  exit 1
fi

REPO="${NINEVEH_DEMO_REPO:-/opt/nineveh/src}"
DIR="${NINEVEH_DEMO_DIR:-/var/lib/nineveh/public}"
PROFILE="${NINEVEH_DEMO_PROFILE:-nineveh-demo}"
NODE="${NINEVEH_DEMO_NODE:-https://api.devnet.aptoslabs.com/v1}"
NETWORK=devnet

cd "$REPO/examples"

# What the page is pointing at now, if anything.
current=""
if [[ -s $DIR/demo.json ]]; then
  current=$(sed -n 's/.*"market": *"\([^"]*\)".*/\1/p' "$DIR/demo.json")
fi

# Still there? Devnet wipes take the contract with them; nothing else does.
#
# Only a 404 counts as gone. A timeout, a 5xx or a rate limit says nothing about the
# contract, and treating one as an answer would republish a contract that was fine and
# move the address out from under every project following it — the exact failure this
# script was rewritten to stop. Anything inconclusive leaves it alone for the next run,
# four hours being nothing against a weekly wipe.
if [[ -n $current ]]; then
  code=000
  for attempt in 1 2 3; do
    # curl writes `000` itself when it never got a response, so the fallback only has
    # to cover it writing nothing at all.
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 \
      -H "Authorization: Bearer $NODE_API_KEY" \
      "$NODE/accounts/$current/module/market" 2>/dev/null) || true
    code=${code:-000}
    [[ $code == 200 || $code == 404 ]] && break
    [[ $attempt -lt 3 ]] && sleep 5
  done

  if [[ $code == 200 ]]; then
    echo "demo: market is still at $current on $NETWORK; nothing to do"
    exit 0
  fi
  if [[ $code != 404 ]]; then
    echo "demo: couldn't tell whether $current is still there (HTTP $code); leaving it alone" >&2
    exit 1
  fi
  echo "demo: nothing at $current any more — devnet was wiped; publishing again"
fi

# Devnet funds over its API, which is the whole reason the demo lives there: no faucet
# page to click through, so this can run unattended.
aptos account fund-with-faucet --profile "$PROFILE" --amount 100000000 >/dev/null 2>&1 || {
  echo "demo: the devnet faucet turned us down; keeping the current deployment" >&2
  exit 1
}

# A build left by a previous run was compiled for a previous address, and a stale one is
# published as-is — which the chain rejects with MODULE_ADDRESS_DOES_NOT_MATCH_SENDER,
# after taking the gas. Start from nothing.
rm -rf 03-market/build

# An object deployment, like examples/deploy.sh: the module's address is baked into its
# bytecode, so each publish compiles against the address it is going to live at.
out=$(aptos move deploy-object \
  --package-dir 03-market \
  --address-name market \
  --profile "$PROFILE" \
  --max-gas 200000 --gas-unit-price 100 \
  --assume-yes 2>&1) || {
  echo "demo: publish failed, keeping the current deployment" >&2
  echo "$out" >&2
  exit 1
}

# The phrase, then the address in it: the output also carries a transaction hash, which
# is the same shape and would be taken instead.
address=$(grep -Eo 'object address 0x[0-9a-f]+' <<<"$out" | grep -Eo '0x[0-9a-f]+' | tail -1)
if [[ -z $address ]]; then
  echo "demo: published but could not find the address in the output" >&2
  echo "$out" >&2
  exit 1
fi

mkdir -p "$DIR"
# Written whole, then moved: the page must never read a half-written file.
cat > "$DIR/demo.json.partial" <<JSON
{
  "network": "$NETWORK",
  "market": "$address",
  "module": "${address}::market",
  "published_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "note": "This address is stable. It only changes when devnet is wiped, which takes the contract with it."
}
JSON
mv "$DIR/demo.json.partial" "$DIR/demo.json"

echo "demo: market published at $address on $NETWORK"
