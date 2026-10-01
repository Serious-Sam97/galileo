# galileo — Go SDK

OpenTelemetry traces, logs and metrics for Go services, exported to Galileo over OTLP/HTTP, with the extras plain
OpenTelemetry lacks: identity (`user.id`, `tenant.id`) on every span, the function and file:line
behind each SQL and Redis call, panics and errors shaped for Issues, log lines linked to their
trace, and LLM-gateway headers.
Inert until an endpoint is configured.

```bash
go get github.com/Serious-Sam97/galileo/sdk/go
```

## Environment

```bash
GALILEO_ENDPOINT=https://galileo-api.serious-sam.dev/otlp   # local: http://localhost:4318
GALILEO_API_KEY=glk_...                                     # project key, ingest scope
OTEL_SERVICE_NAME=my-api
GALILEO_ENV=prod
GALILEO_RELEASE=1.4.2            # service.version (APP_VERSION also works)
GALILEO_SAMPLE_RATIO=1           # optional: keep this fraction of new traces
GALILEO_LOGS=1                   # optional: 0 keeps logs local only
GALILEO_LOG_LEVEL=info           # optional: lowest level sent to Galileo (debug, info, warn, error)
GALILEO_METRICS=1                # optional: 0 sends no metrics
GALILEO_METRICS_INTERVAL=60      # optional: seconds between metric exports
```

## Wiring

Go has no runtime patching, so each piece is connected once, at startup:

```go
tel, err := galileo.Init(ctx, galileo.ConfigFromEnv())
if err != nil { log.Fatal(err) }
defer func() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = tel.Shutdown(ctx) // flush buffered spans
}()

// 1. HTTP: one server span per request, named by route
mux := http.NewServeMux()
handler := tel.Middleware(galileo.WithServeMux(mux))(auth(identify(mux)))

// 2. Identity, after your auth middleware
identify := galileo.Identify(func(r *http.Request) (galileo.Identity, bool) {
	u := currentUser(r.Context())
	if u == nil { return galileo.Identity{}, false }
	return galileo.Identity{UserID: strconv.FormatInt(u.ID, 10), Email: u.Email, TenantID: u.TenantID}, true
})

// 3. Postgres (pgx v5): one span per statement with the calling function
cfg, _ := pgxpool.ParseConfig(os.Getenv("DATABASE_URL"))
cfg.ConnConfig.Tracer = pgxtrace.New()

// 4. Redis (go-redis v9)
rdb.AddHook(redistrace.Hook{})

// 5. Outgoing HTTP: client span + traceparent, so the called service joins the trace
client := &http.Client{Transport: galileo.Transport(nil)}

// 6. Logs (slog): stdout as before, plus Galileo
slog.SetDefault(slog.New(tel.SlogHandler(slog.NewJSONHandler(os.Stdout, nil))))

// 7. Connection pools (optional): connections in use vs the limit, waits
_ = pgxtrace.RecordPoolStats(pool)
_ = redistrace.RecordPoolStats(rdb, "cache")
```

**Logs**: log with the request context (`slog.InfoContext(r.Context(), …)`,
`logger.ErrorContext(ctx, …)`) so the line carries the trace id and the identity set by
`galileo.Identify` (`user.id`, `tenant.id`); without a context it still reaches Logs, unlinked.
`GALILEO_LOG_LEVEL` filters only what goes to Galileo; the local handler keeps its own level.
For zap or zerolog, use the OpenTelemetry bridge for that logger (the provider is installed
globally by `Init`, and identity is added to every record whatever the bridge).

**chi**: `r.Use(tel.Middleware(galileo.WithRoute(chiroute.Pattern)))` as the first middleware,
and `galileo.Identify(...)` after authentication.

**Panics**: the middleware records a panic nobody recovered, marks the request 500 and keeps
panicking. If you recover yourself, call `galileo.RecordPanic(r.Context(), p)` in your recovery.

**Errors**: `galileo.RecordError(ctx, err)` files an exception on the current span (Issues).

**Metrics**: sent every `GALILEO_METRICS_INTERVAL` as deltas (each point is that interval's
count or latency, which is how Galileo charts them). Recorded for every call, **sampled or not**,
so counts and error rates stay exact at `GALILEO_SAMPLE_RATIO=0.2`:

| Metric | Type | Attributes |
| --- | --- | --- |
| `http.server.request.duration` (ms), `http.server.requests` | histogram, counter | `http.route`, `http.method`, `http.status_code`, `tenant.id` |
| `http.server.active_requests` | up-down counter | `http.method` |
| `http.client.request.duration` (ms) | histogram | `http.method`, `server.address`, `http.status_code` or `error.type` |
| `db.client.operation.duration` (ms) | histogram | `db.system.name`, `db.operation.name`, `db.collection.name`, `error.type` |
| `job.duration` (ms) | histogram | `job.name`, `error.type` |
| `db.client.connection.count` / `.max` / `.waits` / `.timeouts` | pool gauges, counters | `db.client.connection.pool.name`, `db.client.connection.state` |
| `go.goroutine.count`, `go.memory.used`, `go.memory.gc.goal`, `go.schedule.duration`, … | runtime | |
| `process.cpu.time`, `system.cpu.time`, `system.memory.usage`, `system.network.io` | host | |

Request metrics use galileo-django's names and units, so one board covers Go and Python services.
Unmatched paths (scanners probing `/wp-login.php`) get no `http.route`, not one series each.
Your own: `galileo.Meter().Int64Counter("orders.placed")`.

**Background work**: `galileo.Job(ctx, "send-reminders", func(ctx context.Context) error { … })`
wraps a job in a span so its queries hang together.

**Other drivers** (`database/sql`, …): `ctx, span := galileo.StartQuery(ctx, "postgresql", q)`
then `galileo.EndQuery(span, err)`.

**Your DB wrapper as call site**: `galileo.SkipCallSitePackages("github.com/acme/shop/internal/db")`
so spans point at the code that called the wrapper.

**LLM gateway**: `req.Header = galileo.GatewayHeaders(ctx, req.Header)` adds `traceparent`,
`x-galileo-user-id` and `x-galileo-tenant-id` to a call to `https://…/gw/v1/chat/completions`.

## Tests

`galileo.Init(ctx, galileo.Config{SpanProcessor: tracetest.NewSpanRecorder()})` records spans in
memory instead of exporting them; `LogProcessor` does the same for log records and
`MetricReader` (a `sdkmetric.ManualReader`) for metrics. With no endpoint and no processor everything is a no-op.
