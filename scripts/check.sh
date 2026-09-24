#!/usr/bin/env bash
# Run everything CI runs, in one pass, and say plainly what failed.
#
# CI is eleven jobs across five tools; running them by hand means running four of them
# and hoping. This runs all of them, keeps going after a failure so one pass shows
# every problem, and exits non-zero if any failed.
#
#   scripts/check.sh            every check
#   scripts/check.sh --fast     the inner loop: fmt, clippy, tests, dependency direction
#   scripts/check.sh --strict   set RUSTFLAGS exactly as CI does (forces a full rebuild)
#   scripts/check.sh --list     what would run
#
# A check whose tool isn't installed is skipped and named, not silently passed.
set -uo pipefail
cd "$(dirname "$0")/.."

# CI's environment, minus RUSTFLAGS: see `--strict`.
export CARGO_TERM_COLOR=always
export CARGO_INCREMENTAL=0
# nineveh-store's queries are checked against the recorded `.sqlx/`, never a live
# database. Setting DATABASE_URL instead would switch sqlx's macros to a real one.
export SQLX_OFFLINE=true

FAST=0
STRICT=0
LIST=0
FORCE=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --strict) STRICT=1 ;;
    --list) LIST=1 ;;
    --force) FORCE=1 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $arg (try --help)" >&2; exit 2 ;;
  esac
done

if [ "$STRICT" = 1 ]; then
  # Exactly CI. This is a different build fingerprint from a plain `cargo build`, so
  # the first run recompiles the world and roughly doubles what target/ holds.
  export RUSTFLAGS="-D warnings"
fi

# --- disk ------------------------------------------------------------------------
# A full check can add several GB to target/. This machine has filled its disk mid-build
# before, and the build is not what you want to lose a session to.
MIN_FREE_GB="${NINEVEH_CHECK_MIN_FREE_GB:-5}"
free_gb() {
  if [ "$(uname)" = "Darwin" ]; then
    df -g . | awk 'NR==2 {print $4}'
  else
    df -BG . | awk 'NR==2 {gsub(/G/,"",$4); print $4}'
  fi
}
FREE="$(free_gb 2>/dev/null || echo 999)"
if [ "$FREE" -lt "$MIN_FREE_GB" ] && [ "$FORCE" = 0 ] && [ "$LIST" = 0 ]; then
  cat >&2 <<EOF
error: ${FREE}GB free, and a full check wants at least ${MIN_FREE_GB}GB.

  target/ regrows to ~20GB after a day of test builds. Free it first:

    cargo clean                 # frees the lot, costs a full rebuild
    rm -rf target/debug/incremental target/release

  Then re-run. To go ahead anyway: scripts/check.sh --force
EOF
  exit 1
fi

# --- the checks ------------------------------------------------------------------
# name|when|tool it needs|command
CHECKS=(
  "fmt|always||cargo fmt --all --check"
  "clippy|always||cargo clippy --workspace --all-targets --locked -- -D warnings"
  "test|always|cargo-nextest|cargo nextest run --workspace --locked --no-tests=pass"
  "doctests|always||cargo test --workspace --doc --locked"
  "deps|always||scripts/check-deps.sh"
  "docs|full||cargo doc --workspace --no-deps --locked"
  "msrv|full|rustup|cargo +1.89 check --locked --all-targets"
  "codegen|full||cargo xtask codegen --check"
  "deny|full|cargo-deny|cargo deny check"
  "studio|full|npm|npm_checks studio"
  "site|full|npm|npm_checks site"
)

# `npm ci` is CI's reproducible install; locally it only earns its cost when the tree
# isn't there, because it deletes and reinstalls node_modules every time.
npm_checks() {
  local dir="$1"
  [ -d "$dir" ] || { echo "no $dir/ directory"; return 0; }
  (
    cd "$dir" || return 1
    if [ ! -d node_modules ]; then
      npm ci --no-audit --no-fund || return 1
    fi
    npm run typecheck && npm run build
  )
}

if [ "$LIST" = 1 ]; then
  for check in "${CHECKS[@]}"; do
    IFS='|' read -r name when _tool cmd <<<"$check"
    [ "$FAST" = 1 ] && [ "$when" = "full" ] && continue
    printf '%-10s %s\n' "$name" "$cmd"
  done
  exit 0
fi

if [ -z "${NINEVEH_TEST_DATABASE_URL:-}" ]; then
  echo "note: NINEVEH_TEST_DATABASE_URL is unset, so the Postgres-backed tests will"
  echo "      skip here and run in CI. Set it to check them:"
  echo "      NINEVEH_TEST_DATABASE_URL=postgres:///nineveh_test scripts/check.sh"
  echo ""
fi

PASSED=()
FAILED=()
SKIPPED=()
START=$SECONDS

for check in "${CHECKS[@]}"; do
  IFS='|' read -r name when tool cmd <<<"$check"
  [ "$FAST" = 1 ] && [ "$when" = "full" ] && continue
  if [ -n "$tool" ] && ! command -v "$tool" >/dev/null 2>&1; then
    SKIPPED+=("$name (needs $tool)")
    continue
  fi
  echo "───── $name ─────"
  at=$SECONDS
  # Broken intra-doc links are warnings, and CI denies them.
  if [ "$name" = "docs" ]; then
    RUSTDOCFLAGS="-D warnings" bash -c "$cmd"
  elif [[ "$cmd" == npm_checks* ]]; then
    $cmd
  else
    bash -c "$cmd"
  fi
  if [ $? -eq 0 ]; then
    PASSED+=("$name")
    echo "  ok ($((SECONDS - at))s)"
  else
    FAILED+=("$name")
    echo "  FAILED ($((SECONDS - at))s)"
  fi
  echo ""
done

# --- what happened ---------------------------------------------------------------
echo "═════ $((SECONDS - START))s ═════"
echo "passed:  ${PASSED[*]:-none}"
[ ${#SKIPPED[@]} -gt 0 ] && printf 'skipped: %s\n' "$(IFS=', '; echo "${SKIPPED[*]}")"
if [ ${#FAILED[@]} -gt 0 ]; then
  printf 'FAILED:  %s\n' "$(IFS=', '; echo "${FAILED[*]}")"
  exit 1
fi
[ "$FAST" = 1 ] && echo "(--fast: docs, msrv, codegen, deny, studio and site weren't run)"
echo "green"
