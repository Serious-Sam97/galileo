# Galileo V2 — Phase 6 (gateway V2)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea + local Ollama.

## Gateway
- [x] cross-format translation of tools, tool calls/results and images (Anthropic Messages ↔ OpenAI Chat), non-streaming and streaming (tool-call argument deltas); tool_choice mapping; unit tests with fixtures both directions
- [x] exact-match prompt cache per route (`budget.cache_ttl_secs`): non-streaming responses cached in memory with TTL/size cap; `x-galileo-cache: hit|miss` header; span attr gen_ai.galileo.cache_hit; cost 0 on hits
- [x] prompt experiments: route `experiment {prompt_name, version_a, version_b, percent_b, sticky_by user}`; picks B for a share of calls when the client asks for the prompt without a version; span records version + experiment name
- [x] feedback + evals: gateway_feedback table + POST /gw/v1/feedback (API key) and POST /projects/{id}/gateway/feedback (session); gateway_evals table + POST /projects/{id}/gateway/evals/run (judge route, sample size, rubric) scoring recent completions 1–5 with an LLM judge; usage endpoint returns avg rating/score per model/route/prompt version
- [x] audio transcription passthrough POST /gw/v1/audio/transcriptions (multipart → OpenAI-compatible provider) and embeddings POST /gw/v1/embeddings, both recorded as gen_ai spans with cost (embedding prices)
- [x] budget warnings: at 80% of daily/monthly USD budget a budget_events row is written once per day; evaluator notifies the route's `budget.alert_recipients` (or project issue recipients) on warn and exhausted
- [x] cargo test + clippy green; server image rebuilt

## UI
- [x] Routes editor: cache TTL, experiment config, budget alert recipients; Usage tab: quality columns (avg rating, judge score) per model/route and a "by prompt version" table; Calls drawer: 👍/👎 + comment (feedback), judge score; Evals sub-tab: run judge, results table
- [x] pnpm build green; web image rebuilt

## Validate
- [x] Anthropic-format tool call served by Ollama (OpenAI format) round-trips; image block translated; cache hit on repeated call; experiment splits versions; feedback stored and visible; eval run scores ≥1 completion; melea Whisper route through gateway records a transcription span (401 from Groq without key is fine, span shows error); docs/gateway.md updated; memory
