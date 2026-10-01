# Browser telemetry (RUM)

`galileo-rum` is a 17 KB script with no dependencies. It reports page loads, SPA route changes,
`fetch`/XHR calls, JavaScript errors and Web Vitals as ordinary OTLP spans, logs and metrics, so
everything lands in the same Query/Traces/Issues views as your backend — and a slow page links to
the backend request that made it slow.

## Install

1. Settings → API keys → create a key with only the **rum** scope. It will be visible in page
   source; `rum` keys can only ingest and everything they send is stamped `galileo.rum = true`.
2. Add the script (served by Galileo itself):

```html
<script src="https://galileo-api.example.com/rum.js"
        data-key="glk_…" data-service="shop-web"
        data-endpoint="https://galileo-api.example.com/otlp"
        data-propagate="https://api.example.com"
        data-env="prod" data-version="2026.09.03"></script>
```

`data-endpoint` defaults to the OTLP receiver next to the script: `/otlp` on the same host
behind the proxy, or port 4318 when the script is served straight from a local `:8080`.

Or, in a bundle: `import "…/rum.js"` and `window.galileoRum.init({ key, service, endpoint, propagate: [...] })`.

3. Let the API accept the trace header from the browser: add `traceparent` (and `tracestate`) to
   the CORS allowed headers of every origin listed in `data-propagate`. Same-origin calls need
   nothing. Django + django-cors-headers: `CORS_ALLOW_HEADERS += ['traceparent', 'tracestate']`.

## What is recorded

| Signal | Where | Fields |
|---|---|---|
| Page load | span `pageload /path` | `rum.type=pageload`, Navigation Timing (`navigation.ttfb_ms`, `dom_content_loaded_ms`, `load_ms`, `transfer_bytes`, `protocol`, `type`), Web Vitals known at send time (`web_vital.lcp|fcp|cls|inp|ttfb`) |
| Route change | span `navigation /path` | `rum.type=navigation`, `navigation.render_ms` |
| fetch / XHR | client span `GET /api/x` | `rum.type=fetch`, `http.request.method`, `http.response.status_code`, `url.full`, `server.address`; error status on network failure or HTTP ≥ 400 |
| JS error | error span `exception TypeError` + ERROR log | `exception.type|message|stacktrace`, `code.filepath`, `code.lineno`; failed resource loads as `ResourceError` |
| Web Vitals | metrics `browser.web_vital.<lcp|cls|inp|fid|fcp|ttfb>` | per page view, sent on pagehide / next navigation |
| Custom | `galileoRum.event("checkout.started", { "cart.items": 3 })`, `galileoRum.log("warn", "…")` | `rum.type=event` |

Every item carries `session.id` (per tab, `sessionStorage`), `url.path`, `page.view` (1, 2, 3…
within the session), `user.id` / `user.email` / `user.name` after `galileoRum.setUser(id, {email, name})`,
and resource attributes `browser.name`, `browser.mobile`, `browser.language`, `screen.*`,
`service.version`, `deployment.environment`.

Fetches are children of the current page-view span (one trace per page view). Requests to the
same origin or to `data-propagate` origins carry `traceparent`, so the API's server span is the
child of the browser's fetch span: Traces shows *page → fetch → view → SQL* in one waterfall.

## UI

- **Browser** (sidebar): Web Vitals p75 with good / needs-improvement / poor thresholds, vitals
  by page, error and failed-fetch counts, and the session list (filter by user).
- **Session view** (`/p/{id}/browser/sessions/{session}`): the user's timeline — pages, the
  fetches under each with the backend span (service, route, status, duration, DB calls) resolved
  from the same trace, errors inline, links to the trace and the issue.
- **User page** lists the user's sessions; a trace with `session.id` links back to its session.

## API

- `GET /api/projects/{id}/sessions?last_seconds&user_id&limit`
- `GET /api/projects/{id}/sessions/{session_id}`
- `GET /api/projects/{id}/rum/vitals?last_seconds`

## Privacy

The script sends URLs (path + query), the user agent, and whatever you pass to `setUser`. It
never reads form fields, cookies or DOM text. Redaction rules apply at ingest like any other
signal, so `url.full` can be masked with a value rule if query strings carry tokens. No session
replay.
