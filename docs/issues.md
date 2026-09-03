# Issues (error tracking)

Exceptions recorded on request spans are grouped into **issues**: one issue per distinct bug,
with first/last seen, counts, affected users, the latest stack trace, and a history of state
changes. It is the Sentry job, built on the traces Galileo already has.

## How grouping works

At ingest, every span carrying an `exception` event gets three hot columns:

| column | meaning |
|---|---|
| `exception_message` | the message, capped at 500 chars |
| `exception_culprit` | the deepest **application** frame of the stack trace as `file:function` (library and framework frames are skipped; falls back to `code.*` when there is no trace) |
| `exception_fingerprint` | hash of `exception_type` + culprit + `http.route` |

Same type, same place, same route → same issue. A different route or a different raising
function is a different issue. Python, Node and Java stack formats are recognised.

## Lifecycle

The alert evaluator rolls up new occurrences per fingerprint every tick:

* unknown fingerprint → issue created (`new`, notifies);
* `open` → counts and last-seen updated;
* `resolved` + new occurrence → back to `open` (`regressed`, notifies), the version it
  reappeared on is recorded;
* `ignored` → counts updated silently.

Resolve, ignore and reopen are manual (UI or API), with an optional note and "fixed in" version.
Notification recipients (webhook / Slack) live in Settings → Issues.

## Deploys

`POST /api/projects/{id}/deploys {"version": "1.4.3", "service": "melea-api", "note": "..."}`
adds a marker that every time-series chart draws as a dashed vertical line. Set `APP_VERSION`
in the app so spans carry `service.version`; issue pages then show which versions an issue
occurred on.

## API

* `GET /api/projects/{id}/issues?status=open|resolved|ignored|all&sort=last_seen|count|users|first_seen&q=&last_seconds=` — with per-issue counts and a 12-bucket sparkline for the window
* `GET /api/projects/{id}/issues/{issue_id}?last_seconds=` — issue, history, occurrence series, breakdowns (routes, users, tenants, versions), recent samples, latest stack trace
* `POST /api/projects/{id}/issues/{issue_id}/resolve|ignore|reopen` `{note, version}`
* `GET|PUT /api/projects/{id}/issue-settings` `{issue_recipients: [{type, url}]}`
* `GET|POST /api/projects/{id}/deploys`

Query language: `exception.type`, `exception.message`, `exception.culprit`, `exception.fingerprint`
are fields, so `COUNT by exception.culprit` or a trigger on a single fingerprint works.
