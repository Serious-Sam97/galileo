# Galileo V3 — Phase 3 (Boards V2 and collaboration)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea in the browser pane.

## Boards V2
- [x] board model v2 (Postgres 0010): `layout` JSONB (grid of panels with x/y/w/h), `variables` JSONB (`[{ name, field, default, values_from }]`), `time_range` JSONB, `compare` bool; panel types: series, table, stat, heatmap, service_map, markdown, issues, slo; migration converts existing boards (one panel per saved query, stacked)
- [x] API: `PUT /boards/{id}/layout`, panel CRUD inside the board JSON, `GET /boards/{id}/variables/{name}/values` (field values in the board's range), `POST /boards/templates/{kind}` creating RED-per-service / LLM cost / browser vitals boards for a project; panel queries accept `$var` substitution server-side (filters and breakdowns)
- [x] UI: drag-and-drop + resize grid (react-grid-layout or a small in-house grid), panel editor drawer (type, query, title), variable pickers in the board header fed by `/variables/{name}/values`, board time range + compare toggle applied to every panel, template picker on the boards list
- [x] pnpm build green; web image rebuilt

## Collaboration
- [x] annotations (Postgres 0010): `annotations(id, project_id, kind: chart|query|board, target JSONB {board_id?, panel_id?, query?, ts?}, at TIMESTAMPTZ, text, mentions UUID[], author, created_at)`; `GET/POST/DELETE /projects/{id}/annotations` (filter by board/panel/time range); @mentions notify the mentioned member through their channels (uses the Phase 5 notifier with an `annotation` kind)
- [x] query history: `query_history(user_id, project_id, query JSONB, text, ran_at)` written by `/query` (deduped per user+text within 1 min, capped at 200 per user); `GET /projects/{id}/query/history` and a History drawer in the Query page with re-run and save
- [x] chart annotations in the UI: click a point on a board/query chart → note with @mention; markers on the chart (markPoint) with hover text; annotation list per board
- [x] PNG render of a board or query result for sharing: `GET /projects/{id}/boards/{id}/render.png` via the web image's headless renderer (playwright-core in the web container) or a server-side ECharts SSR (echarts SSR + resvg) — pick the one that fits the image size; "Copy image" / "Send to channel" (posts the PNG as a Discord/Slack attachment via the notifier)
- [x] cargo test + clippy green; images rebuilt

## Validation + docs
- [x] melea: create a RED board from the template, add a `$tenant` variable, drag panels, compare on; annotate a spike with @ana and see the notification; query history shows the last runs; render.png returns an image and posts to the Discord hook; docs/boards.md; memory; plan archived
