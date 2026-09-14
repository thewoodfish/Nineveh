#!/usr/bin/env bash
# Vendor the Aptos Transaction Stream protos at a pinned aptos-core commit.
#
#   scripts/sync-protos.sh <aptos-core-commit-sha>
#
# Then regenerate the Rust bindings with `cargo xtask codegen` and review the diff.
# We vendor instead of depending on `aptos-protos`: every crates.io release of it is
# yanked, and a git dependency would block publishing to crates.io.
set -euo pipefail

rev="${1:?usage: scripts/sync-protos.sh <aptos-core-commit-sha>}"
if [[ ! "$rev" =~ ^[0-9a-f]{40}$ ]]; then
  echo "error: pass a full 40-character commit SHA, not a branch or tag" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/crates/nineveh-proto/proto"
base="https://raw.githubusercontent.com/aptos-labs/aptos-core/$rev/protos/proto"

files=(
  aptos/indexer/v1/raw_data.proto
  aptos/indexer/v1/filter.proto
  aptos/transaction/v1/transaction.proto
  aptos/util/timestamp/timestamp.proto
)

rm -rf "$dest/aptos"
for f in "${files[@]}"; do
  mkdir -p "$dest/$(dirname "$f")"
  curl --fail --silent --show-error --location "$base/$f" --output "$dest/$f"
done

cat > "$dest/UPSTREAM" <<META
repository: https://github.com/aptos-labs/aptos-core
commit: $rev
path: protos/proto
files:
$(printf '  - %s\n' "${files[@]}")
META

echo "vendored ${#files[@]} protos from aptos-core@$rev"
