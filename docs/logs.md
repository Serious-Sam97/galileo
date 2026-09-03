# Logs: pipelines and log-based metrics

Settings → Logs.

## Pipeline

An ordered list of processors applied to every incoming log record (OTLP, both gRPC and HTTP)
before it is stored. Each processor can be gated by `when: { field, op, value }` (ops: contains,
eq, ne, starts_with, regex, exists, not_exists; fields: `body`, `severity`, `service_name` or any
attribute).

| type | does |
|---|---|
| `json_parse` | body that is a JSON object → attributes (nested keys dotted); `message`/`msg`/`event` becomes the body unless `keep_body` |
| `regex_extract` | named groups of `pattern` on `field` (default body) → attributes |
| `rename` | attribute `from` → `to` |
| `drop` | drops the record when `when` matches (health checks, noisy debug) |
| `severity_map` | first matching `{ pattern, severity }` on the body sets the severity |
| `add_field` | sets a constant attribute |

**Preview** runs the current (unsaved) pipeline over recent logs and shows before/after, dropped
records and the log-based metrics that would fire. Saving invalidates the ingest cache immediately.

API: `GET/PUT /projects/{id}/log-pipeline`, `POST /projects/{id}/log-pipeline/preview`.

## Log-based metrics

`{ name, match, value_from?, unit }` → every matching record writes a gauge point
`log.<name>` (value = 1, or the numeric attribute `value_from`) with `service.name` and
`severity` attributes. Chart it in Query (dataset metrics, `name = log.login_failures`), put it on a
board or a trigger (`SUM(value)` per window). API: `GET/POST/DELETE /projects/{id}/log-metrics`.

## Usage and quotas

Settings → Project shows rows per signal over 7 days with an on-disk estimate (rows × the table's
bytes/row), today's counts, and the highest-cardinality span attributes (candidates for a drop or
rename rule). Quotas are rows per day per signal: `warn` writes an `ingest_quota` event once a day
per signal (delivered to the issue recipients), `hard` makes the OTLP endpoints answer 429 /
RESOURCE_EXHAUSTED for the rest of the day (counters live in memory since the last restart).
API: `GET /projects/{id}/usage`, `PUT /projects/{id}/quotas`.
