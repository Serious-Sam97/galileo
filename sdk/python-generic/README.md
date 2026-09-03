# galileo-python

Galileo for any Python service (FastAPI, Flask, plain scripts, workers).

```bash
pip install galileo-python[fastapi,sqlalchemy]
```

```python
import galileo
galileo.init(endpoint="http://galileo:4318", api_key="glk_...", service="billing", env="prod")

from galileo.fastapi import GalileoMiddleware
app.add_middleware(GalileoMiddleware, identify=lambda scope: {"user_id": scope["state"].user.id})
galileo.sqlalchemy.instrument(engine)          # DB spans with the calling function
galileo.celery.instrument()                    # task spans with identity

@galileo.traced
def price_cart(items): ...
galileo.capture_exception(e)
requests.post(gateway_url, headers=galileo.gateway_headers(), json=...)   # trace + identity + agent run
```

What you get: one server span per request named by the route template with identity, DB spans
with `db.statement`/`db.sql.table` and `code.function.name`/`code.file.path` of the calling
function, client spans (HTTP via the OTel contrib instrumentations) with call sites, logs with
trace ids and `user.id`, exceptions with `exception.*` for Issues, and agent run helpers
(`galileo.agent.run()` / `step()`). Env: `GALILEO_ENDPOINT`, `GALILEO_API_KEY`, `OTEL_SERVICE_NAME`,
`GALILEO_ENV`, `GALILEO_RELEASE`. For Django use `galileo-django`.
