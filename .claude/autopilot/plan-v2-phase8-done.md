# Galileo V2 — Phase 8 (query & UI polish)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea data in the browser pane.

## Query DSL / server
- [x] Derived columns: query `derived: [{ name, expr }]` where expr is a safe arithmetic combo of fields/numbers (+ - * / parens, `duration_ms / gen_ai.usage.output_tokens`); usable as a calculation target and as an order/HAVING field; parser rejects anything but fields, numbers and operators
- [x] HAVING: query `having: [{ field|calc, op, value }]` filtering grouped rows after aggregation; applies to calculations and derived aggregates; cargo test for parse + SQL
- [x] Time comparison: query `compare_to: "previous" | seconds`; run returns a parallel `compare` series aligned to the same buckets (offset applied), for the ghost line; totals include a delta
- [x] Text DSL: `POST /projects/{id}/query/parse` turns `spans | where route = "/x" and status = error | p95(duration_ms), count() by tenant | having count() > 100 | order p95 desc | limit 20` into a Query JSON (and the inverse `stringify` server-side used by tests); errors carry a message + position
- [x] cargo test + clippy green; server image rebuilt (`docker compose build server`)

## Service map
- [x] `GET /projects/{id}/service-map?last_seconds` → nodes (service, span count, error rate, p95) and edges (caller service → callee service, from spans whose parent is in another service), counts + p95 + error rate per edge; DB/LLM/HTTP client spans collapse into their target as leaf nodes

## API extras
- [x] CSV export: `POST /projects/{id}/query/export` streams the current result as text/csv (raw rows or grouped) with a filename; capped rows
- [x] Share links: `POST /projects/{id}/shares` stores a query + frozen absolute time range + view kind, returns a slug; `GET /projects/{id}/shares/{slug}` returns it; org-scoped, read-through project access; migration 0006_shares

## UI
- [x] Query builder: derived-columns editor, HAVING rows, "compare to previous period" toggle drawing a dashed ghost series; CSV download button; "Share" button that freezes the time range and copies a link; a text-DSL box (type the pipe form, parse to the builder, and show the current query as text)
- [x] Service map page `/p/[pid]/services-map` (sidebar "Map"): force/graph of services with edge labels (calls, p95, error %), click a node → query filtered to that service
- [x] Share view: `/p/[pid]/share/[slug]` renders the frozen query read-only (results + charts) with a banner showing the frozen range
- [x] pnpm build green; web image rebuilt (`docker compose build web`)

## Validation + docs
- [x] On melea: derived column cost-per-output-token by model; HAVING count() > N hides small groups; compare-to-previous ghost line on request volume; service map shows melea-api → postgres/groq/ollama; text DSL round-trips a query; CSV downloads; a share link opens read-only with the frozen range; docs/query-dsl.md updated (derived/having/text form/compare), docs for service map + shares; memory
