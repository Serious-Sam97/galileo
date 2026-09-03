# Integrating an app with Galileo

Galileo speaks OpenTelemetry (OTLP) for telemetry and OpenAI/Anthropic-compatible HTTP for LLM
calls. An app needs three things: a **project**, an **API key**, and the **endpoint** of your
Galileo host. Everything else is optional depth.

Hosted example used below: the UI lives on `https://galileo.serious-sam.dev` and everything an
app talks to lives on `https://galileo-api.serious-sam.dev`. Apps only ever need the **API**
hostname. For a local Galileo use `http://localhost` with the raw ports (`:3000` UI, `:8080`
API and gateway, `:4318` OTLP/HTTP, `:4317` OTLP/gRPC).

## 1. Project and key

1. Create a project (org page → New project, or the onboarding wizard at `/p/<id>/welcome`).
2. Settings → API keys → **New key**. Scopes:
   - `ingest` — send traces, logs, metrics (OTLP)
   - `gateway` — call LLMs through `/gw`
   - `rum` — browser/mobile telemetry only (safe to ship to clients)
3. The full key (`glk_…`) is shown once. Keep it in the app's environment, never in a repo.

## 2. Endpoints

| What | Hosted | Local |
| --- | --- | --- |
| UI (people, not apps) | `https://galileo.serious-sam.dev` | `http://localhost:3000` |
| OTLP/HTTP | `https://galileo-api.serious-sam.dev/otlp` (+ `/v1/traces`, `/v1/logs`, `/v1/metrics`) | `http://localhost:4318` |
| OTLP/gRPC | not proxied; use HTTP | `http://localhost:4317` |
| LLM gateway | `https://galileo-api.serious-sam.dev/gw` | `http://localhost:8080/gw` |
| API | `https://galileo-api.serious-sam.dev/api` | `http://localhost:8080/api` |
| Browser script | `https://galileo-api.serious-sam.dev/rum.js` | `http://localhost:8080/rum.js` |

Auth on every call: `Authorization: Bearer glk_…` (OTLP/HTTP also accepts `x-galileo-key`).

Smoke test from any machine:

```bash
curl -s -o /dev/null -w '%{http_code}\n' -X POST https://galileo-api.serious-sam.dev/otlp/v1/traces \
  -H 'content-type: application/json' -H "Authorization: Bearer $GALILEO_API_KEY" \
  -d '{"resourceSpans":[]}'      # 200 = reachable and the key is valid
```

## 3. Pick the SDK for your stack

Every Galileo SDK adds what plain OpenTelemetry lacks: code-level call sites on spans, SQL
spans with the calling function, identity (`user.id`, `tenant.id`) on spans **and** logs,
exceptions as issues, and gateway helpers. All read the same variables:

```bash
GALILEO_ENDPOINT=https://galileo-api.serious-sam.dev/otlp   # Django: GALILEO_OTLP_ENDPOINT
GALILEO_API_KEY=glk_...
GALILEO_ENV=prod                                         # deployment.environment.name
GALILEO_RELEASE=1.4.2                                    # service.version (Django: APP_VERSION)
```

### Django — `galileo-django` (`pip install ./sdk/python`)
```python
INSTALLED_APPS += ["galileo_django"]
MIDDLEWARE = ["galileo_django.middleware.GalileoContextMiddleware", *MIDDLEWARE]
```
Extras: `GALILEO_TRACE_MODULES=app.services,billing.*` turns public functions into spans,
`GALILEO_SQL_PARAMS=1` records redacted query parameters, `GALILEO_FORCE=1` exports from
management commands and workers. Inert unless `GALILEO_OTLP_ENDPOINT` is set. → `docs/sdk-python.md`

### Python (FastAPI, Flask, Celery, scripts) — `galileo` (`pip install ./sdk/python-generic`)
```python
import galileo
galileo.init(endpoint="https://galileo-api.serious-sam.dev/otlp", api_key=KEY, service="my-api", env="prod")
galileo.fastapi.instrument(app)          # or galileo.flask.instrument(app)
galileo.sqlalchemy.instrument(engine)    # SQL spans with call sites
galileo.identity.set(user_id="42", tenant_id="clinic-7")   # in your auth dependency
```
Logging is captured from the root logger; `galileo.capture_exception(e)` files an issue.

### Node (Express, Fastify, Nest, Next) — `@galileo/node` (`sdk/node`)
```bash
node --import @galileo/node/register server.js     # zero-code, reads GALILEO_* env
```
or `init({ endpoint, apiKey, service, env })` as the first import. Auto-instruments http,
express, fastify, pg, mysql, redis; `setIdentity({ userId, tenantId })` per request. → `docs/sdk-node.md`

### PHP / Laravel — `galileo/php` (`sdk/php`, needs Guzzle)
```php
\Galileo\Galileo::init(['endpoint' => 'https://galileo-api.serious-sam.dev/otlp', 'api_key' => $key, 'service' => 'shop', 'env' => 'prod']);
```
Laravel: register `Galileo\Laravel\GalileoServiceProvider`, add the middleware, set
`GALILEO_*` in `.env`; Eloquent queries and Monolog logs are picked up. → `docs/sdk-php.md`

### Rust (axum, sqlx) — `galileo` crate (`sdk/rust/galileo`)
```rust
let _guard = galileo::init(galileo::Config::from_env().service("odeon"));
let app = Router::new().route("/media/{id}", get(media)).layer(axum::middleware::from_fn(galileo::axum::middleware));
let row = galileo::sql!("SELECT * FROM media WHERE id = $1", "postgresql", sqlx::query_as(..).fetch_one(&pool));
```
→ `docs/sdk-rust.md`

### Android (Kotlin) — `galileo-android` (`sdk/android`, AAR)
```kotlin
Galileo.init(this, endpoint = "https://galileo-api.serious-sam.dev/otlp", apiKey = RUM_KEY, service = "momentum-android", env = "prod")
OkHttpClient.Builder().addInterceptor(Galileo.okHttpInterceptor(propagateTo = listOf("https://api.example.com")))
```
Screens, HTTP, Room/SQLite, crashes and ANRs, cold start and frame vitals. Use a `rum`-scoped key. → `docs/sdk-android.md`

### Browser — `galileo-rum.js`
```html
<script src="https://galileo-api.serious-sam.dev/rum.js" data-key="glk_rum_key" data-service="my-site"
        data-endpoint="https://galileo-api.serious-sam.dev/otlp" data-propagate="https://api.example.com"></script>
```
Page loads, navigations, fetches with `traceparent` to your backend (allow the header in your
CORS config), errors, web vitals, sessions. → `docs/rum.md`

### Anything else — plain OpenTelemetry
Any OTel SDK works with the standard variables:
```bash
OTEL_EXPORTER_OTLP_ENDPOINT=https://galileo-api.serious-sam.dev/otlp
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer glk_..."
OTEL_SERVICE_NAME=my-app
OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=prod,service.version=1.4.2
```
Set `user.id` / `tenant.id` on the request span yourself to get identity features. → `docs/connecting.md`

## 4. LLM calls through the gateway

Create a provider (Settings → AI → Providers: Anthropic, OpenAI, Groq, Ollama, any
OpenAI-compatible URL) and a **route** with an alias, e.g. `my-app-chat`, with targets,
fallbacks, budget, cache and guardrails. Then point your client at Galileo and use the alias as
the model:

```python
from openai import OpenAI
client = OpenAI(base_url="https://galileo-api.serious-sam.dev/gw/v1", api_key=GALILEO_API_KEY)
client.chat.completions.create(model="my-app-chat", messages=[...],
    extra_headers={"x-galileo-user": user_id, "x-galileo-conversation": conversation_id})
```

Anthropic SDKs work the same with `base_url=https://galileo-api.serious-sam.dev/gw` (`/v1/messages`).
Embeddings (`/v1/embeddings`) and transcription (`/v1/audio/transcriptions`) go through routes
too. Prompts can be managed in Galileo and referenced by name; feedback lands on
`POST /gw/v1/feedback`. → `docs/gateway.md`, `docs/assistant.md`

## 5. Deploy markers (CI)

```bash
curl -X POST https://galileo-api.serious-sam.dev/api/projects/<project id>/deploys \
  -H "Authorization: Bearer glt_<personal token>" -H 'content-type: application/json' \
  -d '{"version":"1.4.2","service":"my-api","note":"'"$(git log -1 --pretty=%s)"'"}'
```
or `galileo deploy 1.4.2 --service my-api` with the CLI. Charts get a marker; issues record the
version they were first seen on. Send the same string as `service.version`.

## 6. Check it worked

- Overview shows requests within a minute; Traces → open one → the waterfall shows your code
  (`code.function`, file:line) and SQL with the calling function.
- Issues lists exceptions grouped, with users and tenants affected.
- Logs are correlated to traces and carry `user.id`.
- Settings → Connect repeats the snippets with your real key prefix; Settings → Usage shows
  ingest volume and cardinality.

## 7. Production checklist

- Use the `https://…/otlp` endpoint and HTTP/protobuf; gRPC stays internal.
- Separate keys per app and environment (`ingest`, and `gateway` only where needed); `rum` keys
  for anything that ships to users.
- Set `GALILEO_ENV` and a real release string; tail sampling and quotas live in Settings → Project.
- Redaction rules (Settings → Redaction) run at ingest; add one for any attribute that may carry PII.
- Triggers, SLOs, boards, routes and pipelines can be exported as YAML and applied in CI
  (`galileo export` / `galileo import`). → `docs/api.md`
