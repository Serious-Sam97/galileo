# Operating Galileo

## Sampling (tail-based)

Settings → Project → Sampling, or `PUT /api/projects/{id}/settings` with
`{ "sampling": { "rate": 0.2, "keep_errors": true, "slow_ms": 2000, "keep_llm": true, "decision_delay_secs": 10 } }`.

With `rate < 1`, spans are held per trace until the trace has been quiet for `decision_delay_secs`
(or the buffer cap is hit). The whole trace is then kept when any span has an error status, the
root span is slower than `slow_ms`, or the trace carries LLM (`gen_ai.*`) or browser (`session.id`)
spans; otherwise it is kept with probability `rate`, decided from a hash of the trace id so a trace
is never half-kept. Logs are never sampled (they keep their trace id even when the trace was
dropped). `rate = 1` bypasses the buffer entirely. The ingest stats (`/api/system/stats`, Settings →
Galileo health) show `sampled_kept` / `sampled_dropped` / `sampled_buffered`.

Because the decision waits for the trace to finish, the SDK's batch exporter delay (usually up to
5 s) plus `decision_delay_secs` is the added latency before a sampled trace becomes visible.

## Usage and quotas

Settings → Project shows rows and estimated disk per signal over seven days, today's counts and the
highest-cardinality span attributes. Quotas (rows per day per signal) can `warn` (an `ingest_quota`
event delivered to the issue recipients once a day per signal) or be `hard` (the OTLP endpoints answer
429 / RESOURCE_EXHAUSTED for the rest of the day; counted in memory since the last restart). See
[logs.md](logs.md).

## Retention and tiering

`galileo.toml [retention]` sets per-signal TTLs (project overrides in Settings → Project). Data older
than that is deleted by ClickHouse TTL and by the per-project retention job.

To keep cold data cheaply, use a tiered storage policy: hot days on local disk, the rest on S3/MinIO.
Copy [`deploy/clickhouse/storage.xml`](../deploy/clickhouse/storage.xml) into the ClickHouse
container's `/etc/clickhouse-server/config.d/`, fill in the bucket and credentials, mount it in
`docker-compose.yml`, and set

```toml
[retention]
storage_policy = "tiered"   # name of the policy in storage.xml
hot_days = 7                # days kept on the local disk before moving to the cold volume
```

On the next start the schema migration sets `SETTINGS storage_policy = 'tiered'` and
`TTL timestamp + INTERVAL <hot_days> DAY TO VOLUME 'cold'` on spans/logs/metrics (the delete TTL
from `[retention]` still applies after). Queries are transparent; cold reads go through the local
cache disk declared in the policy.

## Backup and restore

```bash
scripts/backup.sh ./backups            # → backups/<UTC stamp>/{clickhouse/,postgres.dump,manifest.json}
scripts/restore.sh backups/<stamp>     # stops galileo-server, restores both stores, starts it again
```

`backup.sh` uses ClickHouse's native `BACKUP TABLE` (zstd) for spans/logs/metrics and `pg_dump -Fc`
for Postgres (orgs, users, keys, triggers, SLOs, issues, gateway config, prompts, shares). Both run
against the compose containers (`galileo-clickhouse`, `galileo-postgres`; override with
`CH_CONTAINER` / `PG_CONTAINER` / `PG_USER` / `PG_DB`). Schedule it with cron or a systemd timer:

```
0 3 * * * cd /opt/galileo && scripts/backup.sh /backups >> /var/log/galileo-backup.log 2>&1
```

- **RPO** = the schedule interval (event data lost since the last backup). Metadata changes are
  rare, so a daily Postgres dump is usually enough; back up more often if retention is short.
- **RTO** ≈ minutes: restore is a table-level `RESTORE` plus a `pg_restore`; the server is stopped
  during it so no writes race the restore.
- Backups on S3: point ClickHouse at an S3 backup disk (`<backups><allowed_disk>`) and change the
  `File(...)` target in the script to `Disk('backups', ...)`; copy `postgres.dump` with `aws s3 cp`.

## Health

`GET /api/system/health` (no auth) returns `ok`, the ClickHouse/Postgres pings, and — for signed-in
org owners at Settings → Galileo health — ingest queue depth vs capacity, rows/s written, drop and
backpressure counts, sampling counters, seconds since the last write, ClickHouse parts per table,
disk used, merges in flight, oldest partition, Postgres pool state, gateway p95 / error rate over the
last 5 minutes, the alert evaluator's last tick age and uptime. Thresholds in the UI: queue > 80 %,
last write > 30 s, parts > 300 per table, evaluator tick > 2 min.

Every 30 s the server also writes `galileo.health.*` gauges into the **Default** project
(`queue_fill`, `rows_per_sec`, `parts`, `evaluator_age_s`, `gateway_p95_ms`), so you can put
triggers and boards on Galileo itself.

## Scale-out (when one server is not enough)

Galileo is designed to run as one server process per deployment. When you outgrow it:

- **Several API/ingest servers** behind a load balancer: all state lives in ClickHouse and Postgres,
  so replicas are stateless except for two in-memory pieces — the tail sampler and the gateway's
  response/semantic caches and health stats. Route OTLP traffic by trace id (an HTTP/2-aware LB
  with hashing on the `traceparent`/`x-galileo-trace-id` header, or one ingest replica) or set
  `sampling.rate = 1` so no buffering happens; gateway caches simply become per-replica.
- **ClickHouse**: keep the schema, switch table engines to `Replicated*MergeTree` on a
  ClickHouse Keeper cluster and put a `Distributed` table in front for sharding; the migrations
  are plain SQL you can adapt (`crates/galileo-storage/src/schema.rs`).
- **Postgres**: any HA setup (Patroni, managed Postgres) — the API only needs a connection string.
- **Evaluator**: run it on exactly one replica (`alerts.enabled = false` on the others) so
  triggers, digests and quota checks are not evaluated twice.
- A compose override with two `server` replicas and an nginx front is a good first step; a Helm
  chart is a straightforward translation of `deploy/docker-compose.yml` (three Deployments, two
  StatefulSets, one Ingress).
