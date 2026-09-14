#!/usr/bin/env bash
# Enforce the workspace's dependency direction (see docs/adr/0005).
#
# Each rule names a crate and dependencies it must never pull in, directly or
# transitively. The pure core stays free of I/O runtimes, and the API layer never
# reaches for the chain. Crates that don't exist yet are skipped, so rules can be
# written ahead of the code.
set -euo pipefail
cd "$(dirname "$0")/.."

rules=(
  "nineveh-core     tokio sqlx tonic"
  "nineveh-proto    tokio sqlx tonic"
  "nineveh-config   tokio sqlx tonic"
  "nineveh-expr     tokio sqlx tonic"
  "nineveh-decode   tokio sqlx"
  "nineveh-engine   tokio sqlx tonic"
  "nineveh-ingest   sqlx"
  "nineveh-store    tonic nineveh-ingest"
  "nineveh-pipeline axum"
  "nineveh-api      nineveh-ingest nineveh-engine"
  "nineveh-realtime nineveh-ingest nineveh-engine"
)

status=0
for rule in "${rules[@]}"; do
  read -r crate forbidden <<<"$rule"
  [[ -d "crates/$crate" ]] || continue
  deps="$(cargo tree --quiet --locked -p "$crate" -e normal --prefix none --format '{p}' | awk '{print $1}' | sort -u)"
  for dep in $forbidden; do
    if grep -qx "$dep" <<<"$deps"; then
      echo "error: $crate depends on $dep, which it must not (see docs/adr/0005)" >&2
      cargo tree --quiet --locked -p "$crate" -e normal -i "$dep" >&2 || true
      status=1
    fi
  done
done

[[ $status -eq 0 ]] && echo "dependency direction ok"
exit $status
