#!/usr/bin/env bash
# Dump the Nineveh database, keep a week of them.
#
# Postgres holds everything: your customers' configs, their state tables, and the
# record log. The configs can be rewritten and the state can be rebuilt, but the record
# log is the one thing that cannot — lose it and every future rebuild becomes a re-read
# of the chain, which is hours per project instead of seconds.
set -euo pipefail

DB="${NINEVEH_BACKUP_DB:-nineveh}"
DIR="${NINEVEH_BACKUP_DIR:-/var/backups/nineveh}"
KEEP_DAYS="${NINEVEH_BACKUP_KEEP_DAYS:-7}"

mkdir -p "$DIR"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
out="$DIR/$DB-$stamp.dump"

# Custom format: compressed, and restorable table by table with pg_restore.
pg_dump --format=custom --compress=6 --file="$out.partial" "$DB"
mv "$out.partial" "$out"

find "$DIR" -name "$DB-*.dump" -mtime "+$KEEP_DAYS" -delete
# A partial file means a dump died half-written; it is not a backup.
find "$DIR" -name "$DB-*.dump.partial" -mtime +1 -delete

echo "wrote $out ($(du -h "$out" | cut -f1)), keeping $KEEP_DAYS days"
