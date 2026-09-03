# Galileo V3 — Phase 4 (logs and metrics maturity)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea logs/spans.

## Log pipelines
- [x] `log_pipelines` (Postgres 0011): per project an ordered list of processors `{ type: json_parse | regex_extract | rename | drop | severity_map | add_field, match?: { field, op, value }, ... }` with enabled flag; `GET/PUT /projects/{id}/log-pipeline`; `POST /projects/{id}/log-pipeline/preview` runs the pipeline over the last N logs and returns before/after
- [x] applied at ingest (OTLP logs path) from the resolver cache like redaction rules: json_parse (body → attributes), regex_extract (named groups → attributes), rename, drop (by match), severity_map (regex on body → severity), add_field; unit tests for every processor; ingest stats count processed/dropped
- [x] log-based metrics: `log_metrics` (Postgres 0011) `{ name, match: {field, op, value}, value_from?: attribute, unit }` → counter/gauge points written to the metrics table at ingest (`log.<name>`), so they chart, trigger and board like any metric
- [x] Settings → Logs tab: pipeline editor (ordered processors, drag to reorder, enable toggle, live preview on recent logs) and log-metrics list

## Span-based metrics (long retention)
- [x] ClickHouse migration V5: `spans_red_1m` (project, minute, service, route, status class, tenant → count, error count, duration p50/p95/p99 via quantilesTDigestState, sum) fed by a materialized view from `spans` root spans; retention 400 days; plus `db_calls_1m` (service, table, operation → count, total ms)
- [x] query engine: when a spans query only uses fields available in the rollup (service_name/http_route/tenant_id/status + COUNT/AVG(is_error)/P50–P99(duration_ms)) and the range is wider than the raw retention or `prefer_rollup` is set, run it against the rollup (merge states); response `stats.source = rollup`; unit tests for the eligibility check
- [x] Overview/boards over 30+ days use the rollup automatically

## Cardinality and usage
- [x] `GET /projects/{id}/usage?last_seconds` — ingested rows/bytes per signal per day, top attribute keys by cardinality (uniq of values) and by row count, estimated storage per signal; org overview shows usage per project
- [x] quotas (Postgres 0011 project_settings.quotas `{ spans_per_day, logs_per_day, metrics_per_day, mode: warn|hard }`): ingest counters per project/day in the writer stats; hard mode returns 429 from OTLP once exceeded; warn mode writes a budget_events-style row `ingest_quota` delivered by the notifier; Settings → Project → Quotas card
- [x] cargo test + clippy green; images rebuilt

## Validation + docs
- [x] melea: a pipeline that json-parses log bodies and extracts `request_id`/`user` shows attributes in Logs; a `log.login_failures` metric charts and a trigger fires on it; a 90-day query on p95 by route answers from the rollup; usage shows melea's volume and `attrs` cardinality; a hard quota of 1000 spans/day returns 429 and the warn mode notifies; docs/logs.md + docs/query-dsl.md (rollups) + docs/operations.md (usage/quotas); memory; plan archived
