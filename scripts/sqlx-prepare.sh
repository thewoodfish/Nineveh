#!/usr/bin/env bash
# Regenerate the offline query data in crates/nineveh-store/.sqlx.
#
# sqlx checks nineveh-store's queries on Nineveh's own tables at compile time. Builds
# without a database (CI, fresh clones) read the recorded results from `.sqlx/`, so
# rerun this whenever a `query!` or a migration changes, and commit the result.
#
# Needs a Postgres server: pass a URL to an empty scratch database, or let it create
# one (`nineveh_sqlx_prepare`) with createdb/dropdb from PATH.
set -euo pipefail
cd "$(dirname "$0")/.."

crate=crates/nineveh-store
url="${1:-}"
if [[ -z "$url" ]]; then
  db=nineveh_sqlx_prepare
  dropdb --if-exists "$db"
  createdb "$db"
  trap 'dropdb --if-exists "$db"' EXIT
  url="postgres:///$db"
fi

for migration in "$crate"/migrations/*.sql; do
  psql -q -v ON_ERROR_STOP=1 -d "$url" -f "$migration"
done

rm -rf "$crate/.sqlx"
mkdir -p "$crate/.sqlx"
# Touch the crate so the macros re-run, and record every query they check.
touch "$crate/src/lib.rs"
SQLX_OFFLINE=false SQLX_OFFLINE_DIR="$PWD/$crate/.sqlx" DATABASE_URL="$url" \
  cargo check --quiet -p nineveh-store --all-targets
echo "wrote $(ls "$crate/.sqlx" | wc -l | tr -d ' ') queries to $crate/.sqlx"
