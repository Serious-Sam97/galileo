# Notifications and alert quality

## Channels

Settings → Notifications. A channel is a reusable destination that triggers, SLO burn alerts,
issue notifications and the daily digest can all use:

| kind | config | delivery |
|---|---|---|
| `slack` | incoming webhook URL | Block Kit message with state, value, threshold and an "Open in Galileo" button |
| `discord` | channel webhook URL | embed, colour by severity |
| `telegram` | `bot_token`, `chat_id` | HTML message via the Bot API |
| `email` | `to: [addresses]` | HTML + text; requires SMTP (`[smtp]` in `galileo.toml` or `GALILEO_SMTP_*` env) |
| `webhook` | URL | raw JSON of the notification |

Every channel has a **Test** button. Recipients on triggers/SLOs/issues are either
`{type: "channel", id}` or, still supported, inline `{type: "webhook"|"slack", url}`.

SMTP for a local sink (Mailpit): `GALILEO_SMTP_HOST=host.docker.internal`, `PORT=1025`,
`SECURITY=none`. Invites are e-mailed automatically when SMTP is configured.

## Triggers

* **Two levels.** `threshold` is critical; optional `warn_threshold` gives a warn state first.
  Notifications fire on every severity change (ok → warn → critical → ok).
* **Sustained for N minutes** (`for_secs`): a breach must hold across evaluations before it
  counts, which removes one-sample spikes.
* **Baseline mode** (`mode: baseline`): instead of a fixed number, the threshold is the same
  query over the same window **7 days ago** × `baseline_factor` + `baseline_min_delta`. Good for
  traffic and error counts with weekly seasonality.
* **Per group** (`per_group`): with breakdowns, each group keeps its own state and notifies on its
  own (`tenant=acme` firing does not hide `tenant=globex`). The detail page lists group states.
* **Mute** for N hours: evaluation continues, notifications pause, state shows `muted`.
* **Logs and issues.** A trigger on `dataset: logs` (e.g. `COUNT` where `severity = error`) is an
  alert on logs. Issues notify through Settings → Issues on *new* and *regressed*.

## Daily digest

Settings → Notifications → Daily digest: channels and the UTC hour. Yesterday's requests,
errors, p95, users, LLM calls and spend, top routes, open issues seen, SLO budgets and trigger
events. **Send now** delivers immediately; **Preview** shows the text.

## API

* `GET|POST /projects/{id}/channels`, `PUT|DELETE …/channels/{channel_id}`, `POST …/channels/{channel_id}/test`
* `GET|PUT /projects/{id}/digest`, `POST /projects/{id}/digest/send`, `GET /projects/{id}/digest/preview`
* trigger fields: `warn_threshold`, `for_secs`, `mute_hours`, `per_group`, `mode`, `baseline_factor`, `baseline_min_delta`; detail returns `groups`

## Detection modes (V3)

- **threshold** — fixed critical/warn values.
- **baseline** — critical = last week's value × factor + delta.
- **anomaly** — the same window at the same time of day over the previous 7 days forms a history;
  the trigger fires when |value − median| > k·MAD (k from `sensitivity` 1–5, loose → tight;
  MAD has a floor of 5 % of the median or half the Poisson noise) and the value is at least
  `min_value`. The trigger page shows the baseline and the band.
- **outlier** — on a per-group trigger, fires for groups whose value is beyond k·MAD of all groups
  in the same evaluation (one tenant behaving unlike the others).
- **composite** — `composite: { all_of | any_of: [trigger ids], within_secs }`: fires when the
  member triggers are firing (or fired within the window) together; the query is ignored.

## Incidents, acknowledgement, repeats

Every transition to warn/critical opens an incident (per group on per-group triggers) and back to
ok closes it. Unacknowledged incidents are re-notified every 30 minutes; notifications carry an
**Acknowledge** link (`/api/ack/<token>`, no login; the page asks for one click to confirm, so mail scanners and chat link previews cannot acknowledge on their own) and the trigger page has an Ack button.
Acknowledging stops repeats and escalation.

## Maintenance windows and mutes

Settings → Notifications → Maintenance windows: a time range (optionally limited to some triggers)
during which matching triggers keep evaluating but do not notify. Per-group mutes
(`POST /triggers/{id}/mute-group {group_key, hours}`) silence one tenant/route for a while.

## On-call and escalation

Settings → Notifications → On-call: a rotation of members (`rotation_days`, `starts_on`) with
escalation steps `[{ after_secs, channel_id }]`. Use `{ type: "oncall", id }` as a trigger
recipient: the current member receives the e-mail; if nobody acknowledges within `after_secs`, the
next step's channel is notified. `GET /oncall` shows who is on call now.
