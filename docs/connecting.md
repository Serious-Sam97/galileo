# Connecting an app to Galileo

Galileo ingests standard OpenTelemetry (OTLP). Nothing Galileo-specific runs inside your app.

## 1. Create an API key

Settings → API keys → New key. Keys look like `glk_…` and carry scopes: `ingest` (OTLP) and
`gateway` (LLM calls). The full key is shown once.

## 2. Point the SDK at Galileo

| Protocol  | Endpoint                     | Auth header                         |
|-----------|------------------------------|-------------------------------------|
| OTLP/gRPC | `http://<host>:4317`         | `Authorization: Bearer glk_…`       |
| OTLP/HTTP | `http://<host>:4318/v1/traces`, `/v1/logs`, `/v1/metrics` | same, or `x-galileo-key: glk_…` |

Every OpenTelemetry SDK reads these environment variables:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer glk_..."
OTEL_SERVICE_NAME=my-app
OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=prod,service.version=1.4.2
```

### Node (Express/Nest/Next)
```bash
npm i @opentelemetry/sdk-node @opentelemetry/auto-instrumentations-node @opentelemetry/exporter-trace-otlp-http
node --require @opentelemetry/auto-instrumentations-node/register app.js
```

### Python (Django/FastAPI)
```bash
pip install opentelemetry-distro opentelemetry-exporter-otlp
opentelemetry-bootstrap -a install
opentelemetry-instrument python manage.py runserver
```

### PHP (Laravel)
Install the `opentelemetry` extension and `open-telemetry/opentelemetry-auto-laravel`; set the
same `OTEL_*` variables in `.env`. Use HTTP (`4318`), which is what the PHP exporter supports.

### Django: use the Galileo SDK instead
`galileo-django` (see `docs/sdk-python.md`) does everything on this page plus code-level
spans, query call sites, identity on logs and clean routes. Two settings lines and the same
`GALILEO_*` variables.

### Rust
See `examples/demo-app`: `opentelemetry-otlp` with `.with_tonic().with_metadata(...)`.

## 3. Add identity to every request

Galileo's "who" columns are `user.id` and `tenant.id`. Set them on the active span in your auth
middleware after login:

```python
from opentelemetry import trace
span = trace.get_current_span()
span.set_attribute("user.id", request.user.id)
span.set_attribute("tenant.id", request.tenant.slug)
```

Everything else the SDK attaches (`http.route`, `http.response.status_code`, `db.system`,
exceptions) is picked up automatically and gets its own fast column.

## 4. Redaction

Default rules drop `*password*`/`*secret*` keys, mask `authorization`/`api_key`, and scrub
bearer tokens and card numbers in values, before anything is stored. Add project rules under
Settings → Redaction (key patterns with drop/mask/hash, or value regexes) and try them on sample
text before saving.

## 5. Check it worked

Overview shows the service within a few seconds. If nothing arrives: `GET /api/system/stats`
shows `auth_failures` (wrong key or missing `ingest` scope) and `rejected_backpressure`.

See also [Browser telemetry (RUM)](rum.md) for the frontend.

SDKs: [Python/Django + any Python](sdk-python.md) · [Node](sdk-node.md) · [PHP/Laravel](sdk-php.md) · [Rust](sdk-rust.md) · [Android](sdk-android.md) · [Browser](rum.md)
