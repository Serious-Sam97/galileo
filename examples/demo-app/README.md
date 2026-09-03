# Galileo demo app

A tiny axum service that shows what a "connected app" looks like:

* OpenTelemetry SDK exporting to Galileo over OTLP/gRPC (`GALILEO_OTLP`, default `http://127.0.0.1:4317`)
  authenticated with a project API key (`GALILEO_KEY`);
* `user.id` / `tenant.id` on every request span;
* an LLM call through the Galileo gateway (`GALILEO_GATEWAY`, default `http://127.0.0.1:8080/gw`)
  with `traceparent` propagated, so the model call lands inside the request trace.

```bash
GALILEO_KEY=glk_... cargo run -p galileo-demo-app
curl localhost:9090/pets/3
curl -X POST localhost:9090/pets/3/describe
```

The gateway route must exist: create a route with alias `assistant` (and a prompt named `greet`
with a `{{tone}}` variable) in Galileo → AI, or adjust `ask_llm`.
