# @galileo/node

OpenTelemetry for Node with the Galileo extras: call-site attribution on DB/HTTP spans, user identity
on every span and log, a console/pino bridge with trace ids, `traced()` for your own functions, and
exception capture that Issues can group.

```bash
npm i @galileo/node
```

```ts
import { init, galileoExpress, traced, captureException, log } from "@galileo/node";
init({ endpoint: "http://galileo:4318", apiKey: "glk_…", service: "shop-api", env: "prod" });

app.use(galileoExpress((req) => req.user && { id: req.user.id, email: req.user.email, tenant: req.tenantId }));
export const chargeCard = traced(async function chargeCard(order) { /* … */ });
```

Zero-code: `node --import @galileo/node/register app.js` with `GALILEO_ENDPOINT`, `GALILEO_API_KEY`,
`OTEL_SERVICE_NAME` (and optional `GALILEO_ENV`, `GALILEO_RELEASE`).

| Feature | How |
|---|---|
| HTTP/Express/Fastify/pg/mysql/redis/mongodb spans | `@opentelemetry/auto-instrumentations-node` (fs/net/dns off by default; pass `instrumentations` to change) |
| `code.function.name` / `code.namespace` / `code.file.path` / `code.line.number` on client spans | stack walk skipping node internals, `node_modules` and the SDK (`callSites: false` to disable, `skipFrames` for extra patterns) |
| `user.id` / `user.email` / `user.name` / `tenant.id` | `setIdentity()`, `runWithIdentity()`, or the Express middleware; propagated with AsyncLocalStorage |
| Logs with `trace_id`/`span_id` and identity | `log(level, msg, attrs)`; `console.*` is bridged by default (`console: false` to disable); pino: `pino({ hooks: { logMethod: galileoPinoHook } })` |
| Exceptions | `captureException(err)`; uncaught exceptions and unhandled rejections are captured automatically |
| Your own spans | `traced(fn, "name")` — sync or async, sets status on throw |

Tests: `npm test` (vitest). Example: `example/app.js`.

**ESM apps**: `--import @galileo/node/register` also registers the OpenTelemetry ESM loader hook so
`import express from "express"` is patched. Route names on the server span (`http.route`) come from
the Express instrumentation; on ESM apps with some Express versions the route is not attached (the
span is `GET` with `url.path`), while CommonJS apps get the full route name. `traced()` spans,
identity, logs and exception capture work the same in both.
