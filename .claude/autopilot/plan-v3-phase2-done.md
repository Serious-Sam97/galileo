# Galileo V3 — Phase 2 (agent observability and control plane V3)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea/Default projects with Ollama.

## Agent runs
- [x] `GET /projects/{id}/agent-runs?last_seconds&user_id` — conversations grouped by `gen_ai.conversation.id`: user, turns (gateway calls), tool calls, tokens, cost, wall time, errors, first/last activity; `GET /projects/{id}/agent-runs/{cid}` — ordered turns with prompt/completion excerpts (if recorded), tool calls (name + args), latency, tokens, cache/guardrail flags, plus app spans from the same traces (tool executions) nested under each turn
- [x] SDK helpers: python `galileo_django.agent.run(name, user)` / `step(name)` context managers that set the conversation id on gateway calls (env/header) and create `agent.step` spans; node `agentRun()` equivalent in `@galileo/node`; docs
- [x] UI: AI → Agents tab (runs table, filter by user) and run page `/p/[pid]/ai/agents/[cid]` (turn timeline: model → tool → model, cost per turn, stalls highlighted) with "Explain this run" via the assistant

## Guardrails
- [x] route `guardrails` config: `{ pii: off|tag|redact|block, injection: off|tag|block, denylist: [regex], max_tokens_per_user_day, max_cost_per_user_day }`; applied before the upstream call: PII (e-mail, card, CPF, phone, secrets) via the redaction engine patterns, prompt-injection heuristics, denylist; per-user daily caps from recorded spans; violations record `gen_ai.galileo.guardrail = <kind>` + `guardrail.action`, block returns 400 in the client's format and is recorded as an error span; unit tests for detection
- [x] Routes editor: Guardrails section; Calls table shows guardrail badges; usage endpoint counts guardrail hits

## Semantic cache
- [x] `budget.semantic_cache = { threshold, ttl_secs, embed_route? }`: near-duplicate prompt matching per route on top of the exact-match cache; embeddings from `embed_route` (gateway /v1/embeddings) when set, otherwise a built-in hashed-token embedding; hits return `x-galileo-cache: semantic`, span attr `gen_ai.galileo.cache_hit = semantic`; unit tests for similarity + eviction

## Datasets and prompt CI
- [x] tables gateway_datasets / gateway_dataset_items (Postgres 0009); `POST/GET /gateway/datasets`, add items from span ids or JSON; prompts gain `ci { dataset_id, judge_route, min_score, required }` and `promoted_version`; creating a prompt version with CI configured runs the judge over the dataset with the new version (prompt injected, target = the route's first target) and stores score/status on the version; `POST /gateway/prompts/{id}/promote` refuses a failing version when `required`; the gateway serves `promoted_version` when the app omits the version and one is promoted
- [x] UI: Prompts tab shows CI config, per-version score/status, Promote; Datasets sub-tab (list, create, add from Calls drawer "add to dataset")

## Smart routing + cost anomaly
- [x] `budget.smart_routing = true`: targets ordered by live health (error rate and p95 over the last 5 min from in-memory stats per target), unhealthy targets (error rate > 50 % with ≥ 5 calls) skipped while others are healthy; span attr `gen_ai.galileo.routing = health`
- [x] cost anomaly job in the evaluator: hourly spend per route vs the same hour over the previous 7 days (median × 3 and ≥ $0.50 above) → budget_events kind `anomaly` → notified to the route's `alert_recipients` / project issue recipients
- [x] cargo test + clippy green; server + web images rebuilt

## Validation + docs
- [x] Default project: a scripted 3-turn agent conversation with a tool call round-trip appears as one run with 3 turns and 1 tool; guardrails on a test route block an injection and redact a CPF/e-mail (span shows the flags); semantic cache hit on a paraphrase; dataset of 4 items + prompt CI run on a new version → score stored, promote works and is refused when failing; smart routing skips the dead provider; a synthetic cost spike creates an anomaly event and a Discord notification
- [x] docs/gateway.md (guardrails, semantic cache, datasets/CI, smart routing, agent runs) + docs/sdk-python.md agent helpers; memory; plan archived
