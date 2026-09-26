#!/usr/bin/env bash
# Keep the examples busy so Studio has something live to show: two accounts count,
# sign the guestbook, trade in the market and play the arena, a transaction every few
# seconds, until Ctrl-C. Run ./deploy.sh first.
#
#   ./play.sh
#   NETWORK=devnet ./play.sh
#
# Whatever `./deploy.sh` published is what this plays with, so `./deploy.sh 03-market`
# then `./play.sh` sends nothing but market traffic.
#
# Set NODE_API_KEY to a Geomi key for the network, or these calls share the anonymous
# per-IP rate limit with everyone else and start failing.
set -uo pipefail
cd "$(dirname "$0")"
NETWORK=${NETWORK:-testnet}
deployed="deployed.$NETWORK.env"
if [[ ! -s $deployed ]]; then
  echo "nothing is published on $NETWORK yet: run ./setup.sh && ./deploy.sh" >&2
  exit 1
fi
# shellcheck source=/dev/null
source "$deployed"
# An address per example, empty for anything not published. `set -u` would otherwise
# abort on the first one missing, which is the common case after `./deploy.sh 03-market`.
COUNTER=${COUNTER:-}
GUESTBOOK=${GUESTBOOK:-}
MARKET=${MARKET:-}
ARENA=${ARENA:-}
A="nineveh-publisher-$NETWORK"
B="nineveh-player-$NETWORK"
echo "playing on $NETWORK:$(
  for one in COUNTER GUESTBOOK MARKET ARENA; do
    [[ -n ${!one} ]] && printf ' %s' "$(echo "$one" | tr '[:upper:]' '[:lower:]')"
  done
)"

run() { # run PROFILE FUNCTION [ARGS...]
  local profile=$1 function=$2
  shift 2
  if aptos move run --profile "$profile" --function-id "$function" "$@" --assume-yes >/dev/null 2>&1; then
    echo "$(date +%T)  ${profile#nineveh-}  ${function#*::}"
  else
    echo "$(date +%T)  ${profile#nineveh-}  ${function#*::}  (failed)"
  fi
}

view() { # view FUNCTION: the first number it returns
  aptos move view --profile "$A" --function-id "$1" 2>/dev/null | grep -Eo '"[0-9]+"' | head -1 | tr -d '"'
}

messages=("gm" "hello from Nineveh" "first!" "nice guestbook" "testing, testing" "wagmi" "who else is here?")
items=("lamp" "rug" "chair" "mug" "poster" "plant" "clock" "kettle")

round=0
while true; do
  round=$((round + 1))
  echo "-- round $round"

  # Counter: both click; the publisher starts over now and then.
  if [[ -n $COUNTER ]]; then
    run $A "$COUNTER::counter::increment"
    run $B "$COUNTER::counter::increment"
    ((round % 10 == 0)) && run $A "$COUNTER::counter::reset"
  fi

  # Arena: both play a random hand.
  if [[ -n $ARENA ]]; then
    run $A "$ARENA::arena::play" --args "u8:$((RANDOM % 3))"
    run $B "$ARENA::arena::play" --args "u8:$((RANDOM % 3))"
  fi

  # Guestbook: the player signs; the publisher replies to some; some are erased.
  if [[ -n $GUESTBOOK ]]; then
    run $B "$GUESTBOOK::guestbook::sign" --args "string:${messages[RANDOM % ${#messages[@]}]}"
    mine=$(($(view "$GUESTBOOK::guestbook::next_id") - 1))
    ((round % 3 == 0)) && run $A "$GUESTBOOK::guestbook::reply" --args "u64:$mine" "string:thanks for signing"
    ((round % 5 == 0)) && run $B "$GUESTBOOK::guestbook::erase" --args "u64:$mine"
  fi

  # Market: credits now and then; one side lists, the other buys most listings.
  if [[ -n $MARKET ]]; then
    ((round % 4 == 1)) && run $A "$MARKET::market::claim_credits" --args u64:5000
    ((round % 4 == 1)) && run $B "$MARKET::market::claim_credits" --args u64:5000
    if ((round % 2 == 0)); then seller=$A buyer=$B; else seller=$B buyer=$A; fi
    run $seller "$MARKET::market::list" --args "string:${items[RANDOM % ${#items[@]}]}" "u64:$((100 + RANDOM % 900))"
    listing=$(($(view "$MARKET::market::next_id") - 1))
    if ((RANDOM % 3 == 0)); then
      run $seller "$MARKET::market::cancel" --args "u64:$listing"
    else
      run $buyer "$MARKET::market::buy" --args "u64:$listing"
    fi
  fi
done
