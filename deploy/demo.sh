#!/usr/bin/env bash
# Keep a fresh copy of the market example on devnet, for the demo page to drive.
#
# Two things make this a recurring job rather than a one-off publish.
#
# Devnet is wiped about weekly, taking every contract with it. And the free tier starts
# a project within six hours of the chain's tip (`crates/nineveh-control/src/tier.rs`),
# so a visitor can only choose "All of its history" — the option that lets a `table:`
# source see the write that created its table — against a contract published in the last
# six hours. A permanent address would quietly give everyone empty listings tables.
#
# So: republish, often, and write where it landed somewhere the page can read.
#
#   NINEVEH_DEMO_DIR=/var/lib/nineveh/public ./demo.sh
#
# It is idempotent in the way that matters: a failure leaves the previous deployment and
# the previous `demo.json` untouched, so the page keeps working on the last good one.
set -euo pipefail

REPO="${NINEVEH_DEMO_REPO:-/opt/nineveh}"
DIR="${NINEVEH_DEMO_DIR:-/var/lib/nineveh/public}"
PROFILE="${NINEVEH_DEMO_PROFILE:-nineveh-demo}"
NETWORK=devnet

cd "$REPO/examples/03-market"

# Devnet funds over its API, which is the whole reason the demo lives there: no faucet
# page to click through, so this can run unattended. Topping up every time is cheaper
# than checking a balance and getting it wrong.
aptos account fund-with-faucet --profile "$PROFILE" --amount 100000000 >/dev/null 2>&1 || {
  echo "demo: the devnet faucet turned us down; keeping the current deployment" >&2
  exit 1
}

# An object deployment, like examples/deploy.sh: the module's address is baked into its
# bytecode, so each publish compiles against the address it is going to live at.
out=$(aptos move deploy-object \
  --profile "$PROFILE" \
  --address-name market \
  --assume-yes 2>&1) || {
  echo "demo: publish failed, keeping the current deployment" >&2
  echo "$out" >&2
  exit 1
}

address=$(echo "$out" | grep -Eo '0x[0-9a-f]{64}' | head -1)
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
  "note": "Republished regularly: devnet is reset weekly, and a free-tier project must start within six hours of the chain tip."
}
JSON
mv "$DIR/demo.json.partial" "$DIR/demo.json"

echo "demo: market published at $address on $NETWORK"
