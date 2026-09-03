# Galileo V3 — Phase 1 (Ask Galileo assistant)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea data with Ollama through the gateway.

## Backend
- [x] org_settings table (Postgres 0008) with `assistant` JSONB `{ enabled, project_id, route_alias, max_steps }`; `GET/PUT /orgs/{id}/assistant` (owner); default: disabled until a route is chosen
- [ ] `Gateway::chat_recorded(project, alias, messages, opts)` — same pipeline as simple_chat but emits a gen_ai span (route, tokens, cost, `gen_ai.galileo.caller = assistant`) and returns text + span/trace ids, so assistant calls show up in the recording project's AI tab and accept feedback
- [x] grounding builder: field catalog (columns + top discovered attrs), services (24h), top http routes, gateway routes, DSL cheat sheet with examples, page context (trace/issue/query/session ids) → system prompt; tool results strip gen_ai.prompt/completion and db.query.parameters and are capped at ~4k chars
- [x] `POST /projects/{id}/assistant/chat` — JSON-action loop (model returns `{"action": run_query|get_trace|bubbleup|list_issues|get_issue|nplusone|deploys|answer, ...}`), up to `max_steps`; tools run under the caller's ProjectAccess (read-only); response has the answer markdown, the steps taken (query text + links), and the recording span/trace/project ids for feedback
- [x] `POST /projects/{id}/assistant/investigate` `{ issue_id | trigger_id }` — deterministic evidence pack (issue/trigger details, occurrences, BubbleUp vs the previous window, sample trace summary, N+1 hits, deploys in range) → one model call → root-cause markdown appended to the issue notes (or returned for triggers)
- [x] `POST /projects/{id}/assistant/explain-trace` `{ trace_id }` — trace summary (critical path, repeated queries, errors, LLM calls, browser page) → markdown
- [x] unit tests: action parsing (fenced/loose JSON), grounding truncation, evidence pack shaping; cargo test + clippy green; server image rebuilt

## UI
- [x] Assistant drawer on every project page (floating button + ⌘J): message list with markdown, "ran query" chips linking to Query, 👍/👎 per answer (gateway feedback on the recording project), page context sent automatically, suggested questions when empty, disabled-state hint linking to settings
- [x] Issue page and trigger page "Investigate" button (streams the write-up into the notes card); trace page "Explain this trace" button; Settings → Organization "Assistant" card (enable, recording project, route alias, max steps, test button)
- [x] pnpm build green; web image rebuilt

## Validation + docs
- [x] melea + Ollama: "which routes were slowest in the last 24h" runs a query and answers with numbers; "why is /api/consultas/ slow" finds the N+1 (get_anexos_count …); Investigate on the InvoiceSyncError issue writes a note; Explain on a trace with 10k queries mentions the repeated statement; assistant calls visible in Default project AI → Calls with `gen_ai.galileo.caller = assistant`; 👍 stored
- [x] docs/assistant.md; README link; memory
