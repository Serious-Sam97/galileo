# Galileo V2 — Phase 5 (notifications and alert quality)

Rules: tick only when it compiles/tests pass. Never git commit/push.

## Server
- [x] notification channels: table notification_channels (project, name, kind webhook|slack|discord|telegram|email, config jsonb, enabled); recipients may be `{type:"channel", id}` or inline (compat); notify module sends email (SMTP from `[smtp]` config), Discord, Telegram, Slack (blocks), webhook; POST …/channels/{id}/test; unit tests for payload builders
- [x] invites send an e-mail when SMTP is configured (link still returned)
- [x] trigger upgrades (migration): warn_threshold, for_secs (breaching_since), mute_until, per_group (trigger_group_states table), mode threshold|baseline (baseline = same window 7d ago × factor, min_delta); evaluator implements warn/critical states, sustained duration, per-group state, mutes; unit tests for the decision function
- [x] daily digest: project_settings digest_channels + digest_hour_utc; job sends yesterday's requests/errors/p95/LLM spend/top issues/SLO budgets/trigger events as HTML e-mail or text
- [x] cargo test + clippy green; server image rebuilt

## UI
- [x] Settings → Notifications: channels CRUD with Test button, digest settings; RecipientsEditor offers channels + inline URLs
- [x] Trigger form: warn threshold, sustained minutes, mode (threshold / baseline vs last week), per-group, mute for N hours; trigger list/detail show warn/critical/muted and per-group states
- [x] pnpm build green; web image rebuilt

## Validate
- [x] on melea: create a Discord-style webhook channel (local receiver) + an e-mail channel against a local SMTP sink (python aiosmtpd/smtpd in Docker or host), test both, trigger fires with warn then critical, muted trigger stays quiet, digest sent on demand (POST …/digest/send); docs/notifications.md + memory
