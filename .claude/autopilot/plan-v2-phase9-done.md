# Galileo V2 — Phase 9 (operations)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea traffic.

## Tail-based sampling
- [x] `project_settings.sampling` JSONB: `{ "rate": 1.0, "keep_errors": true, "slow_ms": 2000, "keep_llm": true, "decision_delay_secs": 10 }` (Postgres 0007); resolver exposes it on ProjectContext (cache refresh honours updates)
- [x] sampler in the ingest writer: spans are buffered per (project, trace) until the trace is quiet for `decision_delay_secs` or the buffer cap is hit; the whole trace is kept when any span is an error, the root exceeds `slow_ms`, or it carries gen_ai/RUM spans; otherwise kept with probability `rate` (deterministic on the trace id, so a trace is never half-kept); logs of dropped traces keep their trace id but are still stored; dropped/kept counters exposed in writer stats; rate 1.0 bypasses buffering entirely
- [x] Settings → Project → "Sampling" card (rate slider, keep errors/slow/LLM toggles) and `PUT /projects/{id}/settings` accepts it; overview shows the sampled share
- [x] unit tests: keep-on-error, keep-on-slow, deterministic rate, flush on quiet; cargo test + clippy green

## Exponential histograms
- [x] ingest keeps exponential-histogram buckets (scale, zero count, positive/negative offset + counts) in the metric row (`exp_scale`, `exp_zero`, `exp_pos_offset`, `exp_pos_counts`, `exp_neg_offset`, `exp_neg_counts`, ClickHouse migration v4) instead of only sum/count/min/max
- [x] HEATMAP and P50–P999 on a metric with exponential buckets use the bucket boundaries (base = 2^(2^-scale)), merged across points; explicit-bucket histograms still work; unit test for bucket→boundary math

## ClickHouse tiering + backup
- [x] `deploy/clickhouse/storage.xml` example (hot local disk + S3/MinIO cold volume, `TTL … TO VOLUME 'cold'`), documented in docs/operations.md with the `galileo.toml [retention]` knobs; migration applies the tiered TTL only when `storage_policy = tiered` is configured (config option `clickhouse.storage_policy`)
- [x] `scripts/backup.sh` (ClickHouse BACKUP … TO Disk/S3 + `pg_dump` of Postgres into one timestamped folder) and `scripts/restore.sh`; both runnable against the compose stack; docs/operations.md describes RPO/RTO and how to schedule

## Self-health
- [x] `GET /system/health` extended: ingest queue depth/capacity, writer batches/s, rows/s, drop counts, sampling kept/dropped, last write age, ClickHouse parts per table + disk used + merges in flight + oldest partition, Postgres pool state, gateway p95 + error rate (last 5 min), alerts evaluator last tick age, uptime
- [x] Settings → "Galileo health" tab (org owners) rendering those with warn/critical thresholds (queue > 80%, last write > 30s, parts > 300, evaluator tick > 2 min); plus a `galileo_health` metric series written every 30 s into the Default project so triggers can alert on Galileo itself
- [x] cargo test + clippy green; server + web images rebuilt (`docker compose build`)

## SDKs
- [x] `sdk/node/` — `@galileo/node` (TypeScript, OTel-based): `init({ endpoint, apiKey, service })`, auto-instrumentation for http/express/fastify/pg/mysql/redis, call-site `code.*` attribution on DB spans (stack walk skipping node_modules), identity from `req.user` via a hook, log bridge (pino/winston/console) with `user.id`/`trace_id`, `traced()` decorator/wrapper, exception capture with `exception.*`; README + tests (vitest) + a tiny example app
- [x] `sdk/php/` — `galileo/php` (Composer, PSR-3 + OTel PHP SDK): Laravel service provider (request span with route name, DB query spans via `DB::listen` with call-site attribution from `debug_backtrace`, identity from `Auth::user()`, log handler for Monolog, exception reporting from the handler), plain-PHP `Galileo::init()`; README + PHPUnit tests
- [x] docs/sdk-node.md, docs/sdk-php.md, docs/connecting.md links; Settings → Connect shows Node and PHP snippets

## Validation + docs
- [x] melea at rate 0.2 with keep-errors/slow: chaos traces and slow consultas are always present, normal traffic is ~20%; overview shows the sampled share; a Python OTel exponential histogram lands and HEATMAP shows its shape; backup.sh + restore.sh round-trip on the compose stack; health tab shows live numbers; Node example app and a Laravel example (palco or a minimal app) send traces with code.* + user.id
- [x] memory updated; plan archived
