# Boards V2 and collaboration

## Boards

A board is a grid of panels (drag to move, drag the corner to resize; 12 columns). Panel types:

| viz | shows |
|---|---|
| `line` | series chart of the panel's query (one line per group/calculation) |
| `table` | grouped totals |
| `stat` | one number (first calculation of the first group) |
| `heatmap` | the query's HEATMAP calculation |
| `markdown` | text (`query.markdown`) |
| `issues` | open issues of the project |
| `slo` | burn/remaining budget of one SLO (`query.slo_id`) |
| `service_map` | the service map for the board's range |

**Variables** (`Settings` on the board): `{ name, field, label, default }`. Panel queries reference
them as `$name` in filter values or breakdowns; the header shows a picker fed by the field's
values in the board's range. A filter whose value is an unset variable is dropped, so a blank
`$tenant` means "all tenants".

**Board time range and compare** apply to every panel (each panel keeps its own range when the
board's is unset). Compare draws the previous window as dashed ghost lines.

**Templates** (Boards → New from template): RED per service (rate, error rate, p50/p95/p99, slowest
routes, DB time by table, open issues, errors by type; `$service`, `$route`), LLM cost & quality
(`$route`, `$model`), Browser vitals (`$page`).

API: `POST /boards/templates {kind, name?, service?}`, `PATCH /boards/{id}/settings
{variables?, time_range?, compare?}`, `GET /boards/{id}/variables/{name}/values?last_seconds&q`,
plus the existing board CRUD (`panels` carries `x/y/w/h/viz`).

## Annotations

Click a point on a board or query chart, write a note, optionally `@user@example.com` a member.
Notes are pinned to that time and appear as markers on every chart of the same board (and on the
Query page for the same query text). Mentions are delivered through the project's issue
recipients (Discord/Slack/e-mail/webhook). API: `GET/POST /annotations`, `DELETE
/annotations/{id}` (author or owner/admin).

## Query history

Every run from the Query page is remembered per user (deduped per minute, last 200). History
drawer → re-run or save. API: `GET /query/history`.

## Share as image

`GET /api/render?pid=…&board=…` (or `&q=<encoded query>&title=…`) on the web app renders the board
server-side (ECharts SVG → PNG) and returns a PNG; the board page has **Image** (download) and
**Send to channel** (posts the PNG to a Discord or webhook channel through
`POST /channels/{id}/send-image?caption=`).
