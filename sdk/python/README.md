# galileo-django

Drop-in observability for Django apps sending to [Galileo](../../README.md).

```bash
pip install galileo-django          # or the wheel in your repo's vendor/ directory
```

```python
# settings.py
INSTALLED_APPS += ['galileo_django']
MIDDLEWARE = ['galileo_django.middleware.GalileoContextMiddleware', *MIDDLEWARE, 'galileo_django.middleware.GalileoViewMiddleware']
```

```bash
GALILEO_OTLP_ENDPOINT=http://localhost:4318
GALILEO_API_KEY=glk_...
OTEL_SERVICE_NAME=my-api
GALILEO_TRACE_MODULES=myapp.services,billing.*     # optional: function spans for these modules
GALILEO_SQL_PARAMS=1                                # optional: redacted SQL parameters on DB spans
```

What you get, with no other code:

* request spans with route (`/api/pets/{pk}/`), user, tenant, request id, sizes, exceptions with stack traces;
* a span per **view** (`pets.views.PetViewSet.retrieve`) and per **database query**, each carrying
  `code.function.name` / `code.file.path` / `code.line.number` of the application code that ran it;
* `db.operation`, `db.table`, `db.row_count` on every query, so N+1 patterns are visible;
* every **log line** tagged with user, tenant, request id, route and function;
* outbound HTTP (including the Galileo LLM gateway) joined to the request trace;
* metrics: request duration/count by route and process metrics.

Add `@galileo_django.traced` (or `traced(capture_args=['pet_id'])`) to service functions you want
as their own spans. Nothing is exported under pytest, from management commands, or when
`GALILEO_OTLP_ENDPOINT` is unset.
