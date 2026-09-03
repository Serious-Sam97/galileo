#!/usr/bin/env bash
# Back up Galileo: ClickHouse tables (native BACKUP) + Postgres (pg_dump) into one timestamped folder.
# Usage: scripts/backup.sh [dest_dir]      (defaults to ./backups)
# Works against the compose stack (containers galileo-clickhouse / galileo-postgres). Override with
# CH_CONTAINER / PG_CONTAINER / PG_USER / PG_DB. Needs `docker`.
set -euo pipefail
DEST=${1:-./backups}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
CH=${CH_CONTAINER:-galileo-clickhouse}
PG=${PG_CONTAINER:-galileo-postgres}
PG_USER=${PG_USER:-galileo}
PG_DB=${PG_DB:-galileo}
OUT="$DEST/$STAMP"
mkdir -p "$OUT"
echo "→ backup $STAMP into $OUT"

# ClickHouse: native BACKUP into /var/lib/clickhouse/backups (the image's backups.allowed_path),
# then copied out of the container.
docker exec "$CH" bash -c "mkdir -p /var/lib/clickhouse/backups/$STAMP && chown -R clickhouse:clickhouse /var/lib/clickhouse/backups"
for t in spans logs metrics galileo_schema_migrations; do
  docker exec "$CH" clickhouse-client -q "BACKUP TABLE galileo.$t TO File('/var/lib/clickhouse/backups/$STAMP/$t') SETTINGS compression_method='zstd'" >/dev/null \
    && echo "  clickhouse: $t"
done
docker cp "$CH:/var/lib/clickhouse/backups/$STAMP" "$OUT/clickhouse"
docker exec "$CH" rm -rf "/var/lib/clickhouse/backups/$STAMP"

# Postgres: metadata (orgs, projects, keys, triggers, issues, gateway config …)
docker exec "$PG" pg_dump -U "$PG_USER" -d "$PG_DB" -Fc > "$OUT/postgres.dump" && echo "  postgres: $(du -h "$OUT/postgres.dump" | cut -f1)"

# manifest
cat > "$OUT/manifest.json" <<JSON
{ "created_at": "$STAMP", "clickhouse_tables": ["spans", "logs", "metrics", "galileo_schema_migrations"], "postgres": "postgres.dump", "galileo_version": "$(docker exec "$PG" true 2>/dev/null && echo compose)" }
JSON
echo "✓ done: $(du -sh "$OUT" | cut -f1)"
