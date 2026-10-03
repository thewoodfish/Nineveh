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

REPO="${NINEVEH_DEMO_REPO:-/opt/nineveh}"
DIR="${NINEVEH_DEMO_DIR:-/var/lib/nineveh/public}"
PROFILE="${NINEVEH_DEMO_PROFILE:-nineveh-demo}"
NETWORK=devnet

# From `examples/`, not from the package: the Aptos CLI finds its profiles in the
# `.aptos/config.yaml` of the directory it runs in, and `--package-dir` is how
# `examples/deploy.sh` points at a contract from there.
cd "$REPO/examples"

# Devnet funds over its API, which is the whole reason the demo lives there: no faucet
# page to click through, so this can run unattended. Topping up every time is cheaper
# than checking a balance and getting it wrong.
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
  "note": "Republished regularly: devnet is reset weekly, and a free-tier project must start within six hours of the chain tip."
}
JSON
mv "$DIR/demo.json.partial" "$DIR/demo.json"

echo "demo: market published at $address on $NETWORK"
