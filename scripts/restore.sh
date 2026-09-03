#!/usr/bin/env bash
# Restore a backup made by scripts/backup.sh into the compose stack. STOPS the galileo-server
# container while restoring so no writes race the restore.
# Usage: scripts/restore.sh backups/<STAMP>
set -euo pipefail
SRC=${1:?backup folder}
CH=${CH_CONTAINER:-galileo-clickhouse}
PG=${PG_CONTAINER:-galileo-postgres}
PG_USER=${PG_USER:-galileo}
PG_DB=${PG_DB:-galileo}
STAMP=$(basename "$SRC")
[ -f "$SRC/postgres.dump" ] || { echo "no postgres.dump in $SRC"; exit 1; }
echo "→ restoring $STAMP (server will be stopped)"
docker stop galileo-server >/dev/null 2>&1 || true

docker exec "$CH" bash -c "mkdir -p /var/lib/clickhouse/backups && chown -R clickhouse:clickhouse /var/lib/clickhouse/backups"
docker cp "$SRC/clickhouse" "$CH:/var/lib/clickhouse/backups/$STAMP"
docker exec "$CH" chown -R clickhouse:clickhouse "/var/lib/clickhouse/backups/$STAMP"
for t in spans logs metrics galileo_schema_migrations; do
  [ -d "$SRC/clickhouse/$t" ] || continue
  docker exec "$CH" clickhouse-client -q "DROP TABLE IF EXISTS galileo.$t" >/dev/null
  docker exec "$CH" clickhouse-client -q "RESTORE TABLE galileo.$t FROM File('/var/lib/clickhouse/backups/$STAMP/$t')" >/dev/null && echo "  clickhouse: $t"
done
docker exec "$CH" rm -rf "/var/lib/clickhouse/backups/$STAMP"

# Postgres: drop + recreate schema, then restore
docker exec "$PG" psql -U "$PG_USER" -d "$PG_DB" -q -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" >/dev/null
docker cp "$SRC/postgres.dump" "$PG:/tmp/restore.dump"
docker exec "$PG" pg_restore -U "$PG_USER" -d "$PG_DB" --no-owner /tmp/restore.dump && echo "  postgres: restored"
docker exec "$PG" rm -f /tmp/restore.dump

docker start galileo-server >/dev/null && echo "✓ server started; Galileo is back at the state of $STAMP"
