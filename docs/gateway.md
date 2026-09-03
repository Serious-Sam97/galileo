# LLM gateway

Point your Anthropic or OpenAI SDK at Galileo and ask for a **route alias** instead of a model
name. Galileo forwards the call, records it as a `gen_ai.*` span, and lets you change the model
behind the alias without redeploying.

## Endpoints

| Client format        | URL                                   |
|----------------------|---------------------------------------|
| Anthropic Messages   | `POST /gw/v1/messages`                |
| OpenAI Chat          | `POST /gw/v1/chat/completions`        |
| Model list (OpenAI)  | `GET /gw/v1/models` → your aliases    |
| Embeddings (OpenAI)  | `POST /gw/v1/embeddings`              |
| Audio → text (OpenAI)| `POST /gw/v1/audio/transcriptions`    |
| End-user feedback    | `POST /gw/v1/feedback`                |

Tools, tool calls / tool results and images are translated between the two chat formats in both
directions, streaming included (tool-call argument deltas). So an Anthropic SDK can call a model
served by an OpenAI-compatible provider (Ollama, Groq, vLLM…) with `tools`, and vice versa.
Embeddings and transcriptions are forwarded as-is to OpenAI-compatible targets and recorded as
`gen_ai.operation.name = embeddings | transcription` spans (embedding prices are built in; audio is
priced per minute when known).

Auth: `x-api-key: glk_…` or `Authorization: Bearer glk_…` (key needs the `gateway` scope).

```python
from anthropic import Anthropic
client = Anthropic(base_url="http://localhost:8080/gw", api_key="glk_...")
client.messages.create(model="assistant", max_tokens=1024, messages=[{"role": "user", "content": "hi"}])
```

```ts
import OpenAI from "openai";
const client = new OpenAI({ baseURL: "http://localhost:8080/gw/v1", apiKey: "glk_..." });
await client.chat.completions.create({ model: "assistant", messages: [{ role: "user", content: "hi" }] });
```

## Providers and routes

* **Provider**: where calls go. Kinds: `anthropic`, `openai`, `ollama`, `openai_compatible`
  (vLLM, Groq, Together, …). Keys are encrypted at rest with the server secret.
* **Route**: an alias → ordered list of `(provider, model)` targets. The first target that
  answers wins; on `429`, `5xx`, timeouts or connection errors the next one is tried and the
  span records `gen_ai.galileo.fallback_index` plus a `gateway.fallback` event per failed
  attempt. Client errors (`400`, `401`, `404`) are returned as-is.
* **Formats**: when the client and the provider speak the same format the request is passed
  through untouched (tools, images, betas all work). When they differ (Anthropic SDK → Ollama,
  OpenAI SDK → Anthropic) Galileo translates text conversations in both directions, streaming
  included.
* **Direct addressing**: `model: "<provider name>/<model>"` bypasses routes (still recorded).

### Limits and budgets

Per route: `requests_per_minute` (token bucket), `daily_usd`, `monthly_usd`, `daily_tokens`.
Spend is read from the recorded spans, so budgets survive restarts. Exceeding one returns a
`429` in the client's error format, and the rejected call is still recorded as an error span.

At 80% of a USD budget and again when it is exhausted, Galileo writes a `budget_events` row (once
per period) and notifies `budget.alert_recipients` on the route (channels, e-mails, webhooks), or
the project's issue recipients when the route has none.

### Response cache

`budget.cache_ttl_secs` turns on an exact-match cache for non-streaming calls on that route:
identical upstream request bodies within the TTL are answered from memory. Hits cost $0, carry the
`x-galileo-cache: hit` header and `gen_ai.galileo.cache_hit = true` on the span.

### What gets recorded

One span per call, `service.name = galileo-gateway`, kind `client`, with:
`gen_ai.system`, `gen_ai.request.model`, `gen_ai.response.model`, `gen_ai.usage.input_tokens`,
`gen_ai.usage.output_tokens`, cache tokens, `gen_ai.usage.cost_usd` (+ `cost_known`),
`gen_ai.response.finish_reasons`, `gen_ai.galileo.route|provider|fallback_index|streaming`,
`gen_ai.galileo.time_to_first_token_ms`, and (unless the route disables it) `gen_ai.prompt` /
`gen_ai.completion` after redaction, capped at 8 KB each.

### Joining the request trace

Send the W3C `traceparent` header of the active span with the gateway call; the gateway span
becomes a child of it. Optional headers `x-galileo-user-id`, `x-galileo-tenant-id`,
`x-galileo-session-id` become `user.id`, `tenant.id`, `gen_ai.conversation.id`.
Response headers `x-galileo-trace-id` / `x-galileo-span-id` link back.

## Prompt registry

AI → Prompts. Each prompt has versions with an optional system prompt, prepended messages and
`{{variables}}`. Ask the gateway to inject one:

```json
{ "model": "assistant", "messages": [...],
  "galileo": { "prompt": { "name": "support-agent", "version": 3, "variables": { "company": "Acme" } } } }
```

Omit `version` for the latest. The span records `gen_ai.galileo.prompt.name` / `.version`, so
you can compare versions in Query (`AVG(is_error)` by `gen_ai.galileo.prompt.version`).

### Experiments (A/B on prompt versions)

A route can carry `budget.experiment = { name, prompt_name, version_a, version_b, percent_b, sticky }`.
When a call asks for that prompt **without** a version, the gateway serves version B to
`percent_b` % of calls (sticky per `x-galileo-user-id` when `sticky` is on) and records
`gen_ai.galileo.experiment` and `gen_ai.galileo.experiment.arm` on the span. Break down cost,
latency, errors, feedback or judge scores by arm to pick the winner, then promote it.

## Feedback and evals

- **End-user feedback** — the app posts `{"span_id", "trace_id", "rating": -1 | 1 | 1..5, "comment", "user_id"}`
  to `POST /gw/v1/feedback` (API key). Span and trace ids come back on every gateway response as
  `x-galileo-span-id` / `x-galileo-trace-id`.
- **Reviewer feedback** — 👍/👎 with a comment in AI → Calls (session API
  `POST /api/projects/{id}/gateway/feedback`).
- **LLM-as-judge** — AI → Evals samples recent successful completions (optionally one route only)
  and asks a *judge route* to score each 1–5 against a rubric (`POST /api/projects/{id}/gateway/evals/run`
  with `judge_route`, `sample_size`, `rubric`, `filter_route`, `last_seconds`). Use a non-thinking
  model as judge so the answer is not spent on reasoning. Scores are stored per span and averaged per
  model, route and prompt version (Usage tab "Score" columns, Evals tab tables,
  `GET /api/projects/{id}/gateway/evals`).

## Pricing

Built-in per-model prices (Anthropic, OpenAI; local models are free). Override per target with
`price_input` / `price_output` (USD per million tokens). Unknown models are flagged
`gen_ai.galileo.cost_known = false`.

## Guardrails

Per route, `budget.guardrails`:

```json
{ "pii": "redact", "injection": "block", "denylist": ["competitor x", "(?i)internal only"],
  "max_tokens_per_user_day": 200000, "max_cost_per_user_day": 2.0 }
```

Checks run on the user turns before the upstream call. `pii` finds e-mails, card numbers, CPF/CNPJ,
phones, IBANs and API keys (`tag` records the hit, `redact` replaces them with `[EMAIL]`-style
markers in the forwarded prompt, `block` rejects with 400). `injection` uses heuristics for
"ignore previous instructions", role-play jailbreaks and pasted system-prompt markers (`tag` or
`block`). `denylist` patterns block by default. Per-user daily caps use the recorded spans for
`x-galileo-user-id`. Every hit is on the span as `gen_ai.galileo.guardrail` (kinds) and
`gen_ai.galileo.guardrail_action`; blocks are recorded as error spans with `error.type = guardrail_*`.

## Semantic cache

`budget.semantic_cache = { "threshold": 0.92, "ttl_secs": 3600 }` serves near-duplicate prompts from
memory on top of the exact-match cache. Only the last user message is embedded, and entries are
bucketed by system prompt and message count, so turn 3 of a chat can never receive turn 2's reply.
The default embedding is a built-in hashed-token model (no network; good for paraphrases sharing
vocabulary) — use a threshold around **0.6–0.7** with it (0.9+ suits real embedding models). Hits answer with `x-galileo-cache: semantic`,
`x-galileo-cache-similarity`, and `gen_ai.galileo.cache_kind = semantic` on the span.

## Smart routing

`budget.smart_routing = true` orders a route's targets by live health (error rate, then p95 over the
last five minutes, kept in memory per target); targets with more than 50 % errors on at least five
calls are tried last. Spans that were reordered carry `gen_ai.galileo.routing = health`.

## Cost anomalies

Every evaluator tick compares each route's spend in the current hour with the same hour over the
previous seven days; when it exceeds three times the median (and at least $0.50 above it) a
`budget_events` row of kind `anomaly` is written once per hour and delivered to the route's
`alert_recipients` (or the project's issue recipients).

## Datasets and prompt CI

AI → Datasets holds golden sets: `{ input: { messages }, expected?, rubric? }` items, added by hand
or from a recorded call (Calls → drawer → add to dataset). A prompt's **CI** picks a dataset, the
route that runs the candidate and the route that judges (1–5), a minimum score and whether promotion
requires passing. Every new prompt version is judged automatically (`ci_status`, `ci_score` on the
version; the run is listed in Evals as kind `prompt_ci`). **Promote** makes a version the one served
when the app omits the version; a required-CI prompt refuses to promote a failing version.

## Agent runs

Gateway calls that share `x-galileo-conversation-id` (or `x-galileo-session-id`) form a run:
AI → Agents lists runs (turns, tool turns, tokens, cost, errors) and the run page shows the
model → tool → model timeline, per-turn prompt/completion excerpts, tool calls and the app's own
spans from the same traces (tool executions, DB calls). SDKs: `galileo_django.agent.run()` /
`agent.step()` (Python) and `agentRun()` / `agentStep()` / `conversationHeaders()` (Node).
