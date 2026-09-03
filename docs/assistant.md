# Ask Galileo (assistant)

An assistant inside the tool that answers questions about your project's data, explains traces
and writes root-cause notes. It runs on **your** gateway: pick a route (Ollama on the Mac works;
a hosted model works too) and every assistant call is recorded like any other gateway call —
cost, latency, tokens and 👍/👎 feedback included.

## Setup

Settings → Organization → **Assistant**: enable, choose the project whose gateway route answers
(also where the assistant's calls are recorded, usually `Default`), the route alias, and the tool
budget per question (default 6 steps). A non-thinking model with solid JSON output works best
(`gemma4:31b-mlx` locally; any Claude/GPT model through a provider).

## What it can do

- **Chat** (floating button, ⌘J): "which routes were slowest yesterday", "why is
  /api/consultas slow", "how many users hit errors last hour by tenant". The model answers by
  running read-only tools — `run_query` (text DSL), `get_trace`, `bubbleup`, `list_issues`,
  `get_issue`, `nplusone`, `deploys` — then writes a short markdown answer. Every query it ran is
  shown as a chip you can open in the Query builder, so answers are reproducible.
- **Investigate** (issue and trigger pages): builds a deterministic evidence pack — the issue or
  trigger, occurrences by route/tenant vs the previous window, BubbleUp of affected spans, N+1
  candidates, a sample trace summary, recent deploys — and asks the model for a root-cause note
  (what is happening, most likely cause, next steps). For issues the note is appended to the
  issue's notes with a timestamp and model name.
- **Explain this trace** (trace page): where the time went, what repeated, what failed, one
  concrete improvement.

## Guardrails

- Tools run under the signed-in user's project access and are read-only.
- Prompts/completions, SQL parameters and stack traces are stripped from tool results before they
  reach the model; results are capped at 4 KB.
- The page context (open trace/issue/query) is sent so answers are relevant; nothing else leaves
  the project.
- Answers carry the recording span id: feedback lands in the recording project's gateway
  feedback and shows up in AI → Evals.

## API

- `POST /api/projects/{id}/assistant/chat` `{ messages: [{role, content}], context: { page, trace_id?, issue_id?, trigger_id?, session_id?, query?, last_seconds } }`
  → `{ answer, steps: [{ action, summary, query_text?, link? }], recording: { project_id, span_id, trace_id, model }, usage }`
- `POST /api/projects/{id}/assistant/investigate` `{ issue_id | trigger_id, last_seconds }` → `{ markdown, … }`
- `POST /api/projects/{id}/assistant/explain-trace` `{ trace_id }` → `{ markdown, … }`
- `GET/PUT /api/orgs/{id}/assistant` → `{ enabled, project_id, route_alias, max_steps }`
