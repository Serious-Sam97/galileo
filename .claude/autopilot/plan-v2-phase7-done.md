# Galileo V2 — Phase 7 (frontend telemetry / RUM)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate with a demo page against melea-api + Galileo.

## Browser script (sdk/browser/galileo-rum.js, served by the server at GET /rum.js)
- [x] vanilla JS, no build step: config from `<script data-key data-service data-endpoint data-propagate data-user>` or `window.galileoRum.init({...})`; session id in sessionStorage (`session.id`), user via `galileoRum.setUser(id, attrs)`
- [x] page load span (`pageload /path`, Navigation Timing: ttfb, dom_interactive, dom_content_loaded, load) + route-change spans (pushState/replaceState/popstate → `navigation /path`), attrs `rum.type`, `url.path`, `url.full`, `browser.*`, `screen.*`, `session.id`, `user.id`
- [x] fetch + XHR instrumentation: `fetch GET /api/x` client spans with http.* attrs and duration; `traceparent` injected for same-origin and `data-propagate` origins, so the backend request span becomes the child of the browser span; response `x-galileo-trace-id` ignored (we own the trace id)
- [x] JS errors (window.onerror, unhandledrejection) → error span with exception.type/message/stacktrace + log record, so Issues groups them (culprit = top frame)
- [x] Web Vitals via PerformanceObserver (LCP, CLS, INP/FID, FCP, TTFB) → metrics `browser.web_vital.<name>` (attrs url.path, session.id) and copied onto the pageload span when known before it is sent (send on `load`+3s or `pagehide`, whichever first)
- [x] batching: queue flushed every 3s / 50 items / pagehide with `fetch(keepalive)`; OTLP/JSON to `<endpoint>/v1/traces|logs|metrics` with `Authorization: Bearer <key>`

## Server
- [x] API-key scope `rum` (ingest-only, meant to be public in page source); UI key creation offers it; OTLP HTTP accepts it for /v1/* and adds `galileo.rum = true` to resource attrs
- [x] CORS on the OTLP HTTP listener (any origin; headers authorization, x-api-key, content-type, traceparent) and on GET /rum.js
- [x] `GET /projects/{id}/sessions?last_seconds&user_id&limit` → sessions grouped by `session.id`: user, first/last seen, pages, fetches, errors, p75 LCP; `GET /projects/{id}/sessions/{sid}` → ordered timeline of RUM spans, each fetch carrying the backend root span (service, route, duration, status, db calls) resolved by trace id; `GET /projects/{id}/rum/vitals?last_seconds` → p75 LCP/CLS/INP/FCP/TTFB by url.path + counts
- [x] field catalog: `session.id`, `rum.type`, `url.path`, `browser.name`, `web_vital.*`; cargo test + clippy green; server image rebuilt

## UI
- [x] Sidebar "Browser" → /p/[pid]/browser: Web Vitals cards (p75 with good/needs-improvement/poor colors), vitals by page table, sessions list (filter by user), errors count
- [x] /p/[pid]/browser/sessions/[sid]: session timeline (pages → fetches with backend span underneath, errors inline, vitals per page), links to traces and issues; users/[userId] page shows the user's sessions; trace view shows "Session" link when `session.id` is present
- [x] Settings → Connect tab gets a "Browser" snippet (`<script src=".../rum.js" data-key data-service>`); pnpm build green; web image rebuilt

## Validation + docs
- [x] melea-api CORS allows `traceparent`/`tracestate` (settings) so browser fetches join the backend trace; examples/rum-demo.html (login to melea, list patients, trigger a JS error) driven in the browser pane; sessions API shows the page → fetch → backend view span chain; Web Vitals recorded; JS error appears in Issues
- [x] docs/rum.md + README + docs/connecting.md links; memory updated
