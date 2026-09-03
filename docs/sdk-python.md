# galileo-django (Python SDK)

`sdk/python/` — the Django integration. It replaces hand-written OpenTelemetry setup with
one app and two middlewares, and adds what plain OpenTelemetry does not give you: the code
location of every query, view spans, identity on every log line, and clean routes.

## Install

```bash
pip install sdk/python                      # from this repo
# or build a wheel and vendor it in the app: cd sdk/python && python -m build --wheel
```

```python
INSTALLED_APPS += ['galileo_django']
MIDDLEWARE = ['galileo_django.middleware.GalileoContextMiddleware', *MIDDLEWARE,
              'galileo_django.middleware.GalileoViewMiddleware']
```

| Env | Meaning |
|---|---|
| `GALILEO_OTLP_ENDPOINT` | `http://host:4318`. Unset = SDK is inert. |
| `GALILEO_API_KEY` | project key with the `ingest` scope |
| `OTEL_SERVICE_NAME` | service name (default: the Django project package) |
| `GALILEO_ENV` | `deployment.environment.name` |
| `GALILEO_TRACE_MODULES` | `app.services,billing.*` — public functions/methods of these modules become spans |
| `GALILEO_SQL_PARAMS=1` | record (redacted) query parameters on DB spans |
| `GALILEO_CAPTURE_HEADERS` | request headers to record (default `user-agent,x-tenant-id,x-request-id`; secrets are masked) |
| `GALILEO_CAPTURE_USER_EMAIL=1` | also record `user.email` |
| `GALILEO_FORCE=1` | export from a management command / worker (off by default) |
| `GALILEO_DISABLED=1` | hard off |

Nothing is exported under pytest.

## What lands on spans

**Request span** (`GET /api/pets/{pk}/`): `http.route` (DRF regexes rewritten), `request.id`,
`user.id`, `tenant.id`, `tenant.domain`, body and response sizes, query-string keys,
allow-listed headers, and on failure the exception with stack trace (even when the exception
comes from middleware).

**View span** (`pets.views.PetViewSet.retrieve`): `code.namespace`, `code.function.name`,
`code.file.path`, `code.line.number`, `django.view.class`, `django.view.args`, renderer.

**Query span** (`SELECT tests_pet`): `db.statement`, `db.operation`, `db.table`, `db.row_count`,
`db.params_count`, optional `db.params`, and the **call site** — `code.*` of the first
application frame on the stack, i.e. the function that ran the query.

**Outbound HTTP** (`requests`, including the Galileo gateway): call site as above, and the
trace context propagated so gateway spans join the request.

**`@traced` spans**: `code.*`, `code.args` (names only), `code.arg.<name>` for allow-listed
values, `code.return_type`, exceptions.

## Logs

Every record emitted during a request carries `request.id`, `http.route`, `http.method`,
`user.id`, `tenant.id`, `tenant.domain`, and the current span's `code.function.name`.
Keys passed via `logger.info(..., extra={...})` become attributes too. One INFO line per
request is emitted by the SDK (`GET /x -> 200 in 12.3ms`).

## Galileo side

New fields: `code.function.name`, `code.namespace`, `code.file.path`, `code.line.number`,
`db.operation`, `db.table`, `request.id` (spans and logs). New endpoints:
`GET /api/projects/{id}/db/nplusone` (repeated statement + call site per trace over a window),
`GET /api/projects/{id}/db/callers?statement=` (who issues a statement), and
`GET /api/projects/{id}/users/{user_id}/timeline` (requests, logs and LLM calls for one user).
The trace view collapses repeated siblings (`SELECT pets ×48`), flags N+1 traces, shows the
"called from" location on query spans, and has a Callers tab.

## Tests

```bash
cd sdk/python && python -m venv .venv && .venv/bin/pip install -e '.[dev]' pytest-django
.venv/bin/python -m pytest
```

## Agent runs

```python
from galileo_django import agent

with agent.run("triage-bot", user=request.user.id) as run:
    reply = provider.chat(messages, headers=run.headers())   # x-galileo-conversation-id + user
    with agent.step("lookup_patient"):
        patient = Patient.objects.get(pk=pk)
```

`run.headers()` (or `agent.conversation_headers()` from anywhere inside the run) makes the gateway
group the calls into one run; steps become `agent.step <name>` spans so the run page shows
model → tool → model.

## Any Python service (`galileo-python`)

Not on Django? `pip install galileo-python[fastapi,sqlalchemy]` (see `sdk/python-generic/README.md`):
`galileo.init(...)`, `GalileoMiddleware` for FastAPI/Starlette (route template names, identity from a
resolver), `galileo.flask.instrument(app)`, `galileo.sqlalchemy.instrument(engine)` (DB spans with the
calling function), `galileo.celery.instrument()`, `@galileo.traced`, `galileo.capture_exception(e)`,
`galileo.gateway_headers()` and the same `galileo.agent.run()` / `step()` helpers.
