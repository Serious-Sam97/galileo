# Go SDK (`github.com/Serious-Sam97/galileo/sdk/go`)

See [sdk/go/README.md](../sdk/go/README.md). Extracted from melea-api-go's `internal/platform/otel`
package and made generic.

| Piece | What it adds |
| --- | --- |
| `galileo.Init(ctx, galileo.ConfigFromEnv())` | tracer provider, OTLP/HTTP exporter, W3C propagation, parent-based sampling; no-op without an endpoint |
| `tel.Middleware(...)` | server span per request named by route (`WithServeMux`, or `chiroute.Pattern` for chi), `http.*`, `x-galileo-trace-id` response header, unrecovered panics |
| `tel.SlogHandler(local)` | slog records to Galileo Logs (and to `local`), with trace id, `user.id` and `tenant.id`; `GALILEO_LOGS=0`, `GALILEO_LOG_LEVEL` |
| metrics (`GALILEO_METRICS`, `GALILEO_METRICS_INTERVAL`) | request, outgoing HTTP, SQL/Redis and job durations for every call (sampled or not), Go runtime, process and host; deltas, ms, galileo-django names |
| `pgxtrace.RecordPoolStats` / `redistrace.RecordPoolStats` | pool connections by state, limit, waits, timeouts |
| `galileo.Identify` / `WithIdentity` | `user.id`, `user.email`, `tenant.id`, `tenant.name` on the request span and every span below it |
| `pgxtrace.New()` | span per pgx statement named `SELECT orders`, `db.query.text` (1 KB, no parameters), call site |
| `redistrace.Hook{}` | span per Redis command or pipeline, call site; misses are not errors |
| `galileo.Transport(nil)` | client span + `traceparent` on outgoing HTTP |
| `galileo.RecordPanic` / `RecordError` | exceptions Issues groups (panics typed `panic`, errors by root cause) |
| `galileo.Job` | span around background work |
| `galileo.GatewayHeaders` | trace and identity headers for LLM gateway calls |

Call sites (`code.function.name`, `code.namespace` = package path, `code.file.path`,
`code.line.number`) are the first stack frame outside the standard library, the instrumented
drivers and this SDK; add your own wrappers with `galileo.SkipCallSitePackages`.

## Moving melea-api-go to the SDK

Done in melea's `internal/platform/otel`, which now only maps melea's configuration, identity,
`sql-audit` hook and Sentry onto the SDK. What changed on the dashboards:

- SQL spans are named `SELECT consultas` instead of `db.query`, and carry `code.*` (the
  repository function; `internal/platform/db` is skipped as a call site) and `db.collection.name`.
- The exporter sends `Authorization: Bearer` instead of `x-galileo-key` (Galileo accepts both).
- `otel.Enrich` became `otel.Identify`, a `galileo.Identify` resolver: `tenant.id`/`tenant.name`/
  `tenant.domain` and `user.id`; `user.email` only with `GALILEO_CAPTURE_USER_EMAIL=1`, as in Django.
- `db.Tracer` is a `pgxtrace.Tracer` whose `Observer` feeds `otel.QueryObserver` (`sql-audit`).
- Each per-clinic job visit runs in `galileo.Job` with the clinic as identity, so job spans, their
  SQL and their logs carry `tenant.id` (a string now, it was an int).
- Sentry stays melea's own, called next to `galileo.RecordPanic` in `httpx.PanicHook`.
- New: logs (`a.Log` wraps the JSON handler with `SlogHandler`), metrics (requests, SQL, jobs,
  the shared pool via `pgxtrace.RecordPoolStats`, runtime, host), and the AI provider's calls go
  through `galileo.Transport` with `GatewayHeaders` on gateway calls (per-clinic AI cost).
- Keep `GALILEO_SAMPLE_RATIO` low on the droplet: metrics still count every request.
