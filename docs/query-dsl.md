# Query DSL

Every chart, saved query, board panel and trigger in Galileo is this JSON structure, sent to
`POST /api/projects/{id}/query`.

```json
{
  "dataset": "spans",
  "time_range": { "last_seconds": 3600 },
  "calculations": [{ "op": "COUNT" }, { "op": "P99", "field": "duration_ms" }],
  "filters": [{ "field": "http.route", "op": "=", "value": "/search" }, { "field": "user.id", "op": "exists" }],
  "filter_combination": "AND",
  "breakdowns": ["tenant.id"],
  "orders": [{ "field": "P99(duration_ms)", "direction": "desc" }],
  "limit": 20,
  "granularity": 60,
  "search": "timeout"
}
```

* **dataset**: `spans` | `logs` | `metrics`.
* **time_range**: `{ "last_seconds": N }` or `{ "start": iso, "end": iso }`.
* **calculations** (VISUALIZE): `COUNT`, `COUNT_DISTINCT(f)`, `SUM/AVG/MIN/MAX(f)`,
  `P50/P75/P90/P95/P99/P999(f)`, `RATE_PER_SEC`, `HEATMAP(f)`. Empty list = raw events mode
  (rows, newest first; `columns` adds extra fields).
* **filters** (WHERE): ops `=`, `!=`, `>`, `>=`, `<`, `<=`, `contains`, `not_contains`,
  `starts_with`, `exists`, `not_exists`, `in`, `not_in`. Combined with `AND` (default) or `OR`.
* **breakdowns** (GROUP BY): up to 8 fields. Results carry totals per group plus a zero-filled
  time series; the series is computed only for the top `limit` groups.
* **granularity**: bucket seconds; auto picks ~120 buckets when omitted.
* **search**: full-text on log bodies (all words must match, index-backed), span name/path, or
  metric name.

## Derived columns, HAVING, comparison

```json
{ "calculations": [{ "op": "SUM", "field": "gen_ai.usage.cost_usd" }, { "op": "SUM", "field": "gen_ai.usage.output_tokens" }, { "op": "COUNT" }],
  "breakdowns": ["gen_ai_model"],
  "derived": [{ "name": "usd_per_1k_out", "expr": "SUM(gen_ai.usage.cost_usd) / SUM(gen_ai.usage.output_tokens) * 1000" }],
  "having": [{ "target": "COUNT", "op": "gt", "value": 5 }],
  "compare_to": "previous" }
```

- **derived** — arithmetic (`+ - * /`, parentheses, numbers) over calculation labels in aggregate
  mode, or over row fields in raw mode. Evaluated after the query, so nothing reaches SQL;
  division by zero yields 0. Derived columns appear after the calculations in results and charts.
- **having** — keeps only groups whose calculation or derived value passes the test
  (`gt gte lt lte eq ne`).
- **compare_to** — `"previous"` (the equal-length window right before this one) or a number of
  seconds. Each group gains `compare_totals` and `compare_series` (re-aligned to the current
  axis), drawn as dashed ghost lines in the UI; `compare_start` / `compare_end` say which window.

## Text form

`POST /api/projects/{id}/query/parse` with `{ "text": "…" }` (and `/query/stringify` for the
inverse). Stages are separated by `|`; the first is the dataset.

```
spans | where http.route = "/api/x" and status_code = error
      | p95(duration_ms), count() by tenant_id
      | having count() > 100 | order p95 desc | limit 20
      | derive per_call = SUM(gen_ai.usage.cost_usd) / COUNT
      | search "timeout" | compare previous
```

Operators: `= != > >= < <= ~ (contains) !~ ^ (starts with) exists not_exists`. Aggregations:
`count() count_distinct(f) sum avg min max p50 p75 p90 p95 p99 p999 heatmap rate`.

## Export and share

- `POST /api/projects/{id}/query/export` runs the query and returns CSV (grouped totals or raw
  rows, capped at 50k rows).
- `POST /api/projects/{id}/shares` `{ query, title }` freezes the time range to absolute
  timestamps and returns a slug; `GET /api/projects/{id}/shares/{slug}` reads it; the UI opens it
  at `/p/{id}/share/{slug}` read-only.

## Service map

`GET /api/projects/{id}/service-map?last_seconds` returns `nodes` (services with span count,
errors, p95, entry-point count), `edges` (caller → callee across services, from parent/child spans
whose `service.name` differs) and `leaves` (DB, LLM and HTTP client calls grouped by target system).
The Map page draws them as a force graph; click a service to query it.

## Rollups (long retention)

Root spans are also aggregated per minute into `spans_red_1m` (service, route, tenant, status →
count, errors, latency quantiles) and DB calls into `db_calls_1m`, kept for 400 days. A spans query
that only uses `service_name`, `http_route`, `tenant_id`, `status_code` (filters `= != in not_in`,
breakdowns) with `COUNT`, `RATE_PER_SEC`, `AVG(is_error)`, `P50…P99(duration_ms)` or
`SUM(duration_ms)` is answered from the rollup automatically when its range is wider than the raw
retention, or when `"prefer_rollup": true` is set. Everything else always reads raw spans.

## Fields

Any attribute key works as a field (`app.cart_size`, `resource.k8s.pod`). Numeric attributes are
stored twice so percentiles and comparisons work without casts. These names resolve to fast
columns and accept the OpenTelemetry semantic-convention spelling:

| Field (aliases)                                  | Notes                       |
|--------------------------------------------------|-----------------------------|
| `service.name`, `service.version`, `deployment.environment.name`, `host.name` | resource |
| `name`, `kind`, `status_code`, `duration_ms`, `duration_s`, `duration_ns` | span |
| `http.request.method`, `http.route`, `http.response.status_code`, `url.path` | http |
| `db.system`, `exception.type`, `user.id`, `tenant.id` |                        |
| `gen_ai.system`, `gen_ai.response.model`, `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`, `gen_ai.usage.cost_usd`, `gen_ai_total_tokens` | LLM |
| `is_root`, `is_error`                            | virtual booleans (0/1)      |
| logs: `severity`, `body`, `trace_id`; metrics: `name`, `value`, `count`, `sum`, `mean` |     |

`GET /api/projects/{id}/fields?dataset=spans` lists columns plus attribute keys seen recently
with counts; `GET .../fields/values?field=tenant.id&q=ac` autocompletes values.

## BubbleUp

`POST /api/projects/{id}/bubbleup` with `{ "query": <query>, "selection": [<filters>] }` compares
the distribution of every attribute for events matching the selection against the rest, ranked
by total variation distance. In the UI, drag on a heatmap or time series, or type a condition.
