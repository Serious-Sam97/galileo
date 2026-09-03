# Node SDK (`@galileo/node`)

See [sdk/node/README.md](../sdk/node/README.md). In short: `init({ endpoint, apiKey, service })` (or
`node --import @galileo/node/register`), auto-instrumentation for http/express/fastify/pg/mysql/redis,
call-site `code.*` on client spans, identity via `setIdentity()` / `galileoExpress()`, console/pino log
bridge with trace ids, `traced()` for your own functions, and exception capture (uncaught + unhandled
rejections) that Issues groups by type + culprit + route.
