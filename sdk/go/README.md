# galileo — Go SDK

OpenTelemetry traces for Go services, exported to Galileo over OTLP/HTTP, with the extras plain
OpenTelemetry lacks: identity (`user.id`, `tenant.id`) on every span, the function and file:line
behind each SQL and Redis call, panics and errors shaped for Issues, and LLM-gateway headers.
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
```

**chi**: `r.Use(tel.Middleware(galileo.WithRoute(chiroute.Pattern)))` as the first middleware,
and `galileo.Identify(...)` after authentication.

**Panics**: the middleware records a panic nobody recovered, marks the request 500 and keeps
panicking. If you recover yourself, call `galileo.RecordPanic(r.Context(), p)` in your recovery.

**Errors**: `galileo.RecordError(ctx, err)` files an exception on the current span (Issues).

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
memory instead of exporting them. With no endpoint and no processor everything is a no-op.

## Not included yet

Logs (slog → OTLP) and runtime metrics. Traces carry the identity; correlate logs by writing the
trace id (`trace.SpanFromContext(ctx).SpanContext().TraceID()`) into your log lines.
