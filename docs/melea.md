# Melea × Galileo (hosted)

How the melea API (`~/Developer/limiar-core/melea-api`, Django, database-per-tenant) reports to
a hosted Galileo, and what changes from the local setup. The UI is at
`https://galileo.serious-sam.dev`; melea talks to `https://galileo-api.serious-sam.dev`.

## What melea already does

- `galileo_django` is installed (settings.py) and stays inert unless `GALILEO_OTLP_ENDPOINT`
  is set; `GalileoContextMiddleware` puts `user.id` and `tenant.id` (the clinic) on every span
  and log line.
- LLM and speech-to-text go through the gateway (`ai/providers/groq.py`): `GALILEO_LLM_ROUTE`
  and `GALILEO_STT_ROUTE` name routes so the model behind them is switched in Galileo, not in code.
- Traced modules: `ai.providers.*`, `tenant_management.provisioning`, `notificacoes.services`;
  SQL parameters are recorded redacted.
- The front-end can send `traceparent`/`tracestate` (allowed in `CORS_ALLOW_HEADERS`) so browser
  sessions link to backend traces.

## 1. Galileo side (once)

1. Project **melea** already exists in the Sam Labs org. On the hosted instance create it again
   (or import the local config: `galileo export --project <local id> > melea.yaml`, then
   `galileo import melea.yaml` against the new host — providers keep `<redacted>` keys, fill
   them in Settings → AI → Providers).
2. Settings → API keys → New key `melea-api production` with scopes `ingest` + `gateway`.
   Optionally a second key `melea-web` with scope `rum` for the browser script.
3. Settings → AI → Routes: `melea-notes` (chat; target Groq/Anthropic/Ollama as you prefer, a
   fallback, daily budget) and `melea-stt` (transcription). Guardrails: PII redaction on for
   `melea-notes` since it sees clinical notes.
4. Optional: Settings → Notifications channel (Discord/e-mail), the RED board from the template
   (Boards → New from template → RED, service `melea-api`), triggers for error rate and p95.

## 2. melea `.env` on the production host

```bash
# Galileo (hosted)
GALILEO_OTLP_ENDPOINT=https://galileo-api.serious-sam.dev/otlp
GALILEO_GATEWAY_URL=https://galileo-api.serious-sam.dev/gw
GALILEO_API_KEY=glk_...                     # the "melea-api production" key
GALILEO_LLM_ROUTE=melea-notes
GALILEO_STT_ROUTE=melea-stt
GALILEO_ENV=prod
GALILEO_TRACE_MODULES=ai.providers.*,tenant_management.provisioning,notificacoes.services
GALILEO_SQL_PARAMS=1
GALILEO_CAPTURE_USER_EMAIL=0                # keep e-mails out of telemetry in prod
APP_VERSION=<git tag or sha>                # becomes service.version on every span

# the DigitalOcean droplet has 1 vCPU: sample instead of switching features off
# (galileo-django >= 0.2.2; measured in docs/overhead.md: ~+10 % CPU at 0.2 with everything on)
GALILEO_SAMPLE_RATIO=0.2                    # keep 1 in 5 traces; raise toward 0.5 when CPU allows
GALILEO_METRICS_INTERVAL=60
```

`/api/consultas/` runs ~218 SQL statements per request (see Traces → N+1 candidates); every
one becomes a span, so fixing that query pattern lowers both the app's CPU and Galileo's.

Local development keeps `http://host.docker.internal:4318` and `http://host.docker.internal:8080/gw`
(see `.env.example`). Only the two URLs, the key and `GALILEO_ENV` differ.

Workers and one-off commands do not export by default; run Celery/cron workers with
`GALILEO_FORCE=1` if you want their spans.

## 3. Deploy marker in the melea pipeline

After a successful deploy:

```bash
curl -fsS -X POST https://galileo-api.serious-sam.dev/api/projects/$GALILEO_PROJECT/deploys \
  -H "Authorization: Bearer $GALILEO_DEPLOY_TOKEN" -H 'content-type: application/json' \
  -d "{\"version\":\"$APP_VERSION\",\"service\":\"melea-api\",\"note\":\"$(git log -1 --pretty=%s)\"}"
```

`GALILEO_DEPLOY_TOKEN` is a personal token (Settings → Personal tokens, `glt_…`) and
`GALILEO_PROJECT` the hosted project id (Settings → Project).

## 4. Browser sessions (optional)

In the melea front-end layout:

```html
<script src="https://galileo-api.serious-sam.dev/rum.js" data-key="glk_<rum key>" data-service="melea-web"
        data-endpoint="https://galileo-api.serious-sam.dev/otlp" data-propagate="https://api.<melea domain>"></script>
```

`CORS_ALLOW_HEADERS` in melea already includes `traceparent` and `tracestate`, so front-end
requests join the backend trace and the Browser page shows the session with its API calls.

## 5. Verify

```bash
# from the melea host: reachable + key valid → 200
curl -s -o /dev/null -w '%{http_code}\n' -X POST https://galileo-api.serious-sam.dev/otlp/v1/traces \
  -H 'content-type: application/json' -H "Authorization: Bearer $GALILEO_API_KEY" -d '{"resourceSpans":[]}'
```

Then hit any melea endpoint and check, in order: Overview (requests), Traces (a
`GET /api/consultas/` trace with `agenda.views…` call sites and SQL), Logs (lines carry
`user.id` and `tenant.id`), AI → Usage (a `melea-notes` call after saving a note), Issues (force
one with the chaos routes if enabled). Ask Galileo (⌘J) "p95 por clínica na última hora" should
answer from real data.

## 6. Hosting Galileo on the two domains

`deploy/docker-compose.prod.yml` puts Caddy in front of the stack and `deploy/Caddyfile` maps
the hostnames to the internal listeners:

| Hostname | Path | Upstream |
| --- | --- | --- |
| `galileo.serious-sam.dev` | `/api/ack/*` (acknowledge links from e-mail and Slack) | `server:8080` |
| `galileo.serious-sam.dev` | everything else | `web:3000` |
| `galileo-api.serious-sam.dev` | `/otlp/*` (prefix stripped) | `server:4318` |
| `galileo-api.serious-sam.dev` | `/api`, `/gw`, `/rum.js` | `server:8080` |

Point both DNS records at the host, open ports 80 and 443, then:

```bash
cd galileo/deploy
cat > .env <<'ENV'
GALILEO_SECRET_KEY=paste-64-hex-from-openssl-rand-hex-32
GALILEO_WEB_DOMAIN=galileo.serious-sam.dev
GALILEO_API_DOMAIN=galileo-api.serious-sam.dev
GALILEO_PUBLIC_URL=https://galileo.serious-sam.dev
GALILEO_PUBLIC_API=https://galileo-api.serious-sam.dev
GALILEO_CORS_ORIGINS=["https://galileo.serious-sam.dev"]
ENV
docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d --build
```

Caddy requests the certificates on first start. ClickHouse and Postgres stay bound to localhost.
Backups: `scripts/backup.sh` (see `docs/operations.md`).

Why each variable matters:
- `GALILEO_PUBLIC_API` is **baked into the web image at build time**, so rebuild `web` whenever
  it changes: `docker compose ... up -d --build web`.
- `GALILEO_PUBLIC_URL` is the UI address the server writes into notification links and the SSO
  landing page. It must be the web domain, not the API one.
- `GALILEO_CORS_ORIGINS` lists the browser origins allowed to call the API with credentials.
  Without the UI origin in it, login fails in the browser.
- The session cookie is set by the API host with `SameSite=Lax` and, because the public URL is
  https, `Secure`. Both names sit under `serious-sam.dev`, so the browser treats UI → API calls
  as same-site and sends it. If you ever move the API to a different registrable domain, the
  cookie stops being sent and the UI needs a same-origin proxy instead.
- SSO redirect URL: `https://galileo-api.serious-sam.dev/api/auth/oidc/callback`.
- OTLP gRPC (4317) is not exposed publicly; every SDK here uses OTLP/HTTP.

### Behind a Cloudflare Tunnel

If Cloudflare terminates TLS (Tunnel or proxied DNS), Caddy must not run ACME and the tunnel
must point **both hostnames at Caddy's port 80**, never at the API or web ports directly:
the `/otlp/*` route and the acknowledge-link route live in Caddy, so bypassing it turns every
OTLP request into a 404 from the API. Mount `deploy/Caddyfile.cloudflare` instead of
`Caddyfile` (plain `http://` site blocks, `auto_https off`) and reload Caddy. Tunnel ingress:

```yaml
ingress:
  - hostname: galileo.serious-sam.dev
    service: http://localhost:80
  - hostname: galileo-api.serious-sam.dev
    service: http://localhost:80
  - service: http_status:404
```

### Verifying the host

```bash
curl -s https://galileo-api.serious-sam.dev/api/system/health     # {"ok":true,...}
curl -sI https://galileo.serious-sam.dev | head -1                # 200 from the UI
```
