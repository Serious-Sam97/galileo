# Go SDK (`github.com/Serious-Sam97/galileo/sdk/go`)

See [sdk/go/README.md](../sdk/go/README.md). Extracted from melea-api-go's `internal/platform/otel`
package and made generic.

| Piece | What it adds |
| --- | --- |
| `galileo.Init(ctx, galileo.ConfigFromEnv())` | tracer provider, OTLP/HTTP exporter, W3C propagation, parent-based sampling; no-op without an endpoint |
| `tel.Middleware(...)` | server span per request named by route (`WithServeMux`, or `chiroute.Pattern` for chi), `http.*`, `x-galileo-trace-id` response header, unrecovered panics |
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

melea's package maps one to one, with these differences to check on the dashboards:

- SQL spans are named `SELECT consultas` instead of `db.query`, and carry `code.*` and
  `db.collection.name`. Redis spans are `redis get` instead of `cache.get`.
- The exporter sends `Authorization: Bearer` instead of `x-galileo-key` (Galileo accepts both).
- Sentry is not part of the SDK: keep melea's Sentry init and call it next to
  `galileo.RecordPanic` in `httpx.PanicHook`.
- `otel.Enrich` becomes a `galileo.Identify` resolver reading `tenant.FromContext` and
  `auth.PrincipalFromContext` (put `tenant.domain` in `Identity.Attributes`).
- `db.Tracer = queryTracer{…}` becomes `db.Tracer = &pgxtrace.Tracer{Observer: …}` (the `sql-audit`
  hook moves to `Observer`), and `cache` uses `redistrace.Hook{}`.
- The per-clinic job loop can use `galileo.Job`.
