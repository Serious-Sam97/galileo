# Galileo V3 — Phase 5 (monitoring V2)

Rules: tick only when it compiles/tests pass. Never git commit/push. Validate on melea data.

## Detection modes
- [x] anomaly mode for triggers: `mode = anomaly` with `sensitivity` (1–5) — baseline = median of the same time-of-day over the previous 7 days (and same weekday when 4+ weeks exist), deviation = MAD; fires when |value − baseline| > k·MAD (k from sensitivity) and the value is above `min_value`; evaluator computes the baseline with one ClickHouse query per trigger; preview shows baseline band + value
- [x] outlier mode: `mode = outlier` on a per_group trigger — fires for groups whose value is beyond k·MAD of the other groups in the same evaluation (e.g. one tenant's error rate vs the rest); state per group like today
- [x] composite triggers: `composite = { all_of|any_of: [trigger ids], within_secs }` evaluated after the members; fires when the member conditions hold within the window; notified like a trigger
- [x] unit tests for baseline/MAD math and composite logic; docs/notifications.md update

## Incident flow
- [x] maintenance windows (Postgres 0012): `{ name, starts_at, ends_at, filters?: {service?, route?}, triggers?: [ids] }` — matching triggers do not fire/notify inside the window (state still tracked); Settings → Notifications card; API CRUD
- [x] mute by attribute on a per_group trigger: `mutes: [{ group_key, until }]` (silence one tenant/route); `POST /triggers/{id}/mute-group`
- [x] incident timeline: `trigger_incidents` (fired_at, acknowledged_at/by, resolved_at, peak_value, notes) opened when a trigger fires and closed when it returns to ok; `POST /triggers/{id}/ack`; acknowledgement stops repeat notifications; the trigger page shows the incidents list and the notification e-mail/Discord embed carries an "Acknowledge" link (signed token, no login needed)
- [x] on-call: `oncall_schedules` (Postgres 0012): rotation `{ name, members: [user ids], rotation_days, starts_on, escalation: [{ after_secs, channel_id }] }`; a trigger can target `{ type: "oncall", id }` as a recipient → the notifier resolves the current on-call member's e-mail (and escalates to the next channel when not acknowledged after `after_secs`); Settings → Notifications → On-call editor with "who is on call now"
- [x] cargo test + clippy green; images rebuilt

## Validation + docs
- [x] melea: an anomaly trigger on request volume previews a baseline band; an outlier trigger per tenant flags the chaos tenant; a maintenance window silences a firing trigger; ack from the Discord link stops repeats; an on-call schedule with two members escalates to the webhook channel after 60 s without ack; docs/notifications.md (modes, incidents, on-call); memory; plan archived
