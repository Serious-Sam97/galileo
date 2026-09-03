//! Daily digest: yesterday's numbers per project, sent once a day at the configured UTC hour
//! to the project's digest channels. `send_now` backs the "send now" button.

use chrono::{Duration, NaiveDate, Utc};
use galileo_core::ProjectId;
use galileo_storage::SqlQuery;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::notify::{html_escape, Notification};
use crate::Evaluator;

pub struct Digest {
    pub project_name: String,
    pub date: NaiveDate,
    pub requests: f64,
    pub errors: f64,
    pub p95_ms: f64,
    pub users: f64,
    pub llm_calls: f64,
    pub llm_cost_usd: f64,
    pub top_issues: Vec<(String, i64)>,
    pub slo_lines: Vec<String>,
    pub trigger_events: i64,
    pub top_routes: Vec<(String, f64, f64)>,
}

pub async fn build(ev: &Evaluator, project: Uuid, date: NaiveDate) -> anyhow::Result<Digest> {
    let start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + Duration::days(1);
    let (name,): (String,) = sqlx::query_as("SELECT name FROM projects WHERE id = $1").bind(project).fetch_one(&ev.pg).await?;
    let f = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0);
    let base = "FROM spans WHERE project_id = ? AND timestamp >= fromUnixTimestamp64Nano(?) AND timestamp < fromUnixTimestamp64Nano(?)";
    let params = || vec![ProjectId(project).into(), start.into(), end.into()];
    let tot = ev.storage.query(&SqlQuery { sql: format!("SELECT countIf(parent_span_id = '' AND http_route != ''), countIf(parent_span_id = '' AND status_code = 'error'), quantileTDigestIf(0.95)(duration_ns, parent_span_id = '') / 1e6, uniqIf(user_id, user_id != ''), countIf(gen_ai_system != ''), sum(gen_ai_cost_usd) {base}"), params: params() }).await?;
    let r = tot.rows.first().cloned().unwrap_or_default();
    let routes = ev.storage.query(&SqlQuery { sql: format!("SELECT http_route, count(), quantileTDigest(0.95)(duration_ns) / 1e6 {base} AND parent_span_id = '' AND http_route != '' GROUP BY http_route ORDER BY count() DESC LIMIT 5"), params: params() }).await?;
    let issues: Vec<(String, i64)> = sqlx::query_as("SELECT title, count FROM issues WHERE project_id = $1 AND status = 'open' AND last_seen >= $2 ORDER BY last_seen DESC LIMIT 5").bind(project).bind(start).fetch_all(&ev.pg).await?;
    let slos: Vec<(String, Value)> = sqlx::query_as("SELECT name, coalesce(last_result, '{}'::jsonb) FROM slos WHERE project_id = $1 ORDER BY name").bind(project).fetch_all(&ev.pg).await?;
    let (trigger_events,): (i64,) = sqlx::query_as("SELECT count(*) FROM trigger_events e JOIN triggers t ON t.id = e.trigger_id WHERE t.project_id = $1 AND e.fired_at >= $2 AND e.fired_at < $3").bind(project).bind(start).bind(end).fetch_one(&ev.pg).await?;
    Ok(Digest {
        project_name: name,
        date,
        requests: r.first().map(f).unwrap_or(0.0),
        errors: r.get(1).map(f).unwrap_or(0.0),
        p95_ms: r.get(2).map(f).unwrap_or(0.0),
        users: r.get(3).map(f).unwrap_or(0.0),
        llm_calls: r.get(4).map(f).unwrap_or(0.0),
        llm_cost_usd: r.get(5).map(f).unwrap_or(0.0),
        top_issues: issues,
        slo_lines: slos.into_iter().map(|(n, v)| format!("{n}: SLI {} vs target {}%, budget left {}", v.get("sli_pct").and_then(|x| x.as_f64()).map(|x| format!("{x:.3}%")).unwrap_or_else(|| "n/a".into()), v.get("target_pct").and_then(|x| x.as_f64()).unwrap_or(0.0), v.get("budget_remaining_pct").and_then(|x| x.as_f64()).map(|x| format!("{x:.1}%")).unwrap_or_else(|| "n/a".into()))).collect(),
        trigger_events,
        top_routes: routes.rows.iter().map(|row| (row[0].as_str().unwrap_or("").to_string(), f(&row[1]), f(&row[2]))).collect(),
    })
}

pub fn render_text(d: &Digest) -> String {
    let err_pct = if d.requests > 0.0 { d.errors / d.requests * 100.0 } else { 0.0 };
    let mut t = format!("{} — {}\nRequests {:.0} · errors {:.0} ({err_pct:.2}%) · p95 {:.0} ms · users {:.0}\nLLM calls {:.0} · ${:.4}\nTrigger events: {}\n", d.project_name, d.date, d.requests, d.errors, d.p95_ms, d.users, d.llm_calls, d.llm_cost_usd, d.trigger_events);
    if !d.top_routes.is_empty() { t.push_str("\nTop routes:\n"); for (r, n, p) in &d.top_routes { t.push_str(&format!("  {r} — {n:.0} req, p95 {p:.0} ms\n")); } }
    if !d.top_issues.is_empty() { t.push_str("\nOpen issues seen:\n"); for (title, n) in &d.top_issues { t.push_str(&format!("  {title} ({n})\n")); } }
    if !d.slo_lines.is_empty() { t.push_str("\nSLOs:\n"); for l in &d.slo_lines { t.push_str(&format!("  {l}\n")); } }
    t
}

pub fn render_html(d: &Digest, url: &str) -> String {
    let err_pct = if d.requests > 0.0 { d.errors / d.requests * 100.0 } else { 0.0 };
    let stat = |label: &str, v: String| format!("<td style=\"padding:8px 12px;border:1px solid #eee\"><div style=\"font-size:11px;color:#888;text-transform:uppercase\">{label}</div><div style=\"font-size:20px;font-weight:600\">{v}</div></td>");
    let mut h = format!("<div style=\"font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;max-width:680px;margin:0 auto;padding:24px\"><h2 style=\"margin:0\">{} — daily digest</h2><div style=\"color:#888\">{}</div><table style=\"border-collapse:collapse;margin:16px 0\"><tr>{}{}{}{}{}{}</tr></table>",
        html_escape(&d.project_name), d.date, stat("requests", format!("{:.0}", d.requests)), stat("errors", format!("{:.0} ({err_pct:.2}%)", d.errors)), stat("p95", format!("{:.0} ms", d.p95_ms)), stat("users", format!("{:.0}", d.users)), stat("LLM calls", format!("{:.0}", d.llm_calls)), stat("LLM spend", format!("${:.4}", d.llm_cost_usd)));
    if !d.top_routes.is_empty() { h.push_str("<h3>Top routes</h3><ul>"); for (r, n, p) in &d.top_routes { h.push_str(&format!("<li><code>{}</code> — {n:.0} requests, p95 {p:.0} ms</li>", html_escape(r))); } h.push_str("</ul>"); }
    if !d.top_issues.is_empty() { h.push_str("<h3>Open issues seen</h3><ul>"); for (t, n) in &d.top_issues { h.push_str(&format!("<li>{} <span style=\"color:#888\">({n})</span></li>", html_escape(t))); } h.push_str("</ul>"); }
    if !d.slo_lines.is_empty() { h.push_str("<h3>SLOs</h3><ul>"); for l in &d.slo_lines { h.push_str(&format!("<li>{}</li>", html_escape(l))); } h.push_str("</ul>"); }
    h.push_str(&format!("<p>{} trigger event(s).</p><p><a href=\"{url}\" style=\"background:#f5a524;color:#1a1200;padding:8px 14px;border-radius:6px;text-decoration:none\">Open Galileo</a></p></div>", d.trigger_events));
    h
}

pub async fn send(ev: &Evaluator, project: Uuid, channels: &Value, date: NaiveDate) -> anyhow::Result<usize> {
    let d = build(ev, project, date).await?;
    let url = format!("{}/p/{}/overview", ev.public_url, project);
    let text = render_text(&d);
    let targets = crate::notify::resolve(&ev.pg, project, &crate::notify::parse_recipients(channels)).await;
    let mut ok = 0;
    for t in &targets {
        let res = match t {
            crate::notify::Target::Email(to) => ev.mailer.send(to, &format!("[Galileo] {} — digest for {}", d.project_name, d.date), &render_html(&d, &url), &text).await,
            other => crate::notify::send_target(&ev.http, &ev.mailer, other, &Notification { kind: "digest", state: "info".into(), name: d.project_name.clone(), project_id: project, title: format!("{} — daily digest {}", d.project_name, d.date), message: text.clone(), value: None, threshold: None, url: Some(url.clone()), at: Utc::now() }).await,
        };
        match res { Ok(()) => ok += 1, Err(e) => tracing::warn!(error = %e, "digest send failed") }
    }
    Ok(ok)
}

pub async fn run_due(ev: &Evaluator) -> anyhow::Result<()> {
    let now = Utc::now();
    let today = now.date_naive();
    let rows: Vec<(Uuid, Value, i32, Option<NaiveDate>)> = sqlx::query_as("SELECT project_id, digest_channels, digest_hour_utc, digest_last_sent FROM project_settings WHERE jsonb_array_length(digest_channels) > 0").fetch_all(&ev.pg).await?;
    for (project, channels, hour, last) in rows {
        if last == Some(today) || (now.format("%H").to_string().parse::<i32>().unwrap_or(0)) < hour { continue; }
        let sent = send(ev, project, &channels, today - Duration::days(1)).await.unwrap_or(0);
        let _ = json!(sent);
        sqlx::query("UPDATE project_settings SET digest_last_sent = $2 WHERE project_id = $1").bind(project).bind(today).execute(&ev.pg).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders() {
        let d = Digest { project_name: "melea".into(), date: NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(), requests: 100.0, errors: 3.0, p95_ms: 120.0, users: 7.0, llm_calls: 5.0, llm_cost_usd: 0.01, top_issues: vec![("X: y".into(), 3)], slo_lines: vec!["a".into()], trigger_events: 2, top_routes: vec![("/api/x/".into(), 50.0, 90.0)] };
        let t = render_text(&d);
        assert!(t.contains("3.00%") && t.contains("/api/x/"));
        assert!(render_html(&d, "http://g").contains("daily digest"));
    }
}
