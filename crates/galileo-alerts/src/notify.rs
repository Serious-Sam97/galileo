//! Outbound notifications. A recipient is either inline (`{type: "webhook"|"slack", url}`) or a
//! reference to a project channel (`{type: "channel", id}`), which may be a webhook, Slack,
//! Discord, Telegram or e-mail channel. Everything is best-effort and logged.

use galileo_core::config::SmtpConfig;
use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Recipient {
    Webhook { url: String },
    Slack { url: String },
    Channel { id: Uuid },
    /// The current on-call member of a schedule (e-mail).
    Oncall { id: Uuid },
}

/// A resolved destination.
#[derive(Debug, Clone)]
pub enum Target {
    Webhook(String),
    Slack(String),
    Discord(String),
    Telegram { bot_token: String, chat_id: String },
    Email(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub kind: &'static str,
    pub state: String,
    pub name: String,
    pub project_id: Uuid,
    pub title: String,
    pub message: String,
    pub value: Option<f64>,
    pub threshold: Option<f64>,
    pub url: Option<String>,
    pub at: chrono::DateTime<chrono::Utc>,
}

pub fn parse_recipients(v: &Value) -> Vec<Recipient> {
    v.as_array().map(|a| a.iter().filter_map(|r| serde_json::from_value(r.clone()).ok()).collect()).unwrap_or_default()
}

pub fn target_from_channel(kind: &str, config: &Value) -> Option<Target> {
    let s = |k: &str| config.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    Some(match kind {
        "webhook" => Target::Webhook(s("url")?),
        "slack" => Target::Slack(s("url")?),
        "discord" => Target::Discord(s("url")?),
        "telegram" => Target::Telegram { bot_token: s("bot_token")?, chat_id: s("chat_id")? },
        "email" => Target::Email(config.get("to")?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()),
        _ => return None,
    })
}

/// Turn recipients into targets, loading channels from Postgres.
pub async fn resolve(pg: &PgPool, project: Uuid, recipients: &[Recipient]) -> Vec<Target> {
    let mut out = Vec::new();
    for r in recipients {
        match r {
            Recipient::Webhook { url } => out.push(Target::Webhook(url.clone())),
            Recipient::Slack { url } => out.push(Target::Slack(url.clone())),
            Recipient::Channel { id } => {
                let row: Option<(String, Value, bool)> = sqlx::query_as("SELECT kind, config, enabled FROM notification_channels WHERE id = $1 AND project_id = $2")
                    .bind(id).bind(project).fetch_optional(pg).await.unwrap_or(None);
                if let Some((kind, config, true)) = row {
                    if let Some(t) = target_from_channel(&kind, &config) { out.push(t); }
                }
            }
            Recipient::Oncall { id } => {
                if let Some((_, email)) = crate::incidents::current_oncall(pg, *id, chrono::Utc::now()).await { if !email.is_empty() { out.push(Target::Email(vec![email])); } }
            }
        }
    }
    out
}

fn emoji(n: &Notification) -> &'static str {
    match n.state.as_str() {
        "triggered" | "critical" | "regressed" | "new" | "exhausted" | "burning" => "🚨",
        "warn" => "⚠️",
        "ok" | "resolved" => "✅",
        _ => "ℹ️",
    }
}

pub fn slack_payload(n: &Notification) -> Value {
    let mut fields = vec![json!({ "type": "mrkdwn", "text": format!("*State*\n{}", n.state) })];
    if let Some(v) = n.value { fields.push(json!({ "type": "mrkdwn", "text": format!("*Value*\n{v:.3}") })); }
    if let Some(t) = n.threshold { fields.push(json!({ "type": "mrkdwn", "text": format!("*Threshold*\n{t}") })); }
    let mut blocks = vec![
        json!({ "type": "header", "text": { "type": "plain_text", "text": format!("{} {}", emoji(n), n.title).chars().take(150).collect::<String>() } }),
        json!({ "type": "section", "text": { "type": "mrkdwn", "text": n.message } }),
        json!({ "type": "section", "fields": fields }),
    ];
    if let Some(u) = &n.url { blocks.push(json!({ "type": "actions", "elements": [{ "type": "button", "text": { "type": "plain_text", "text": "Open in Galileo" }, "url": u }] })); }
    json!({ "text": format!("{} {} — {}", emoji(n), n.title, n.state), "blocks": blocks })
}

pub fn discord_payload(n: &Notification) -> Value {
    let color = match n.state.as_str() { "ok" | "resolved" => 0x3ecf8e, "warn" => 0xf5a524, _ => 0xff5c6c };
    let mut fields = vec![json!({ "name": "State", "value": n.state, "inline": true })];
    if let Some(v) = n.value { fields.push(json!({ "name": "Value", "value": format!("{v:.3}"), "inline": true })); }
    if let Some(t) = n.threshold { fields.push(json!({ "name": "Threshold", "value": t.to_string(), "inline": true })); }
    json!({ "embeds": [{ "title": format!("{} {}", emoji(n), n.title), "description": n.message, "color": color, "fields": fields, "url": n.url, "timestamp": n.at.to_rfc3339() }] })
}

pub fn telegram_text(n: &Notification) -> String {
    let mut t = format!("{} <b>{}</b> — {}\n{}", emoji(n), html_escape(&n.title), n.state, html_escape(&n.message));
    if let Some(v) = n.value { t.push_str(&format!("\nvalue: {v:.3}")); }
    if let Some(th) = n.threshold { t.push_str(&format!(" · threshold: {th}")); }
    if let Some(u) = &n.url { t.push_str(&format!("\n{u}")); }
    t
}

pub fn email_html(n: &Notification) -> String {
    let color = match n.state.as_str() { "ok" | "resolved" => "#3ecf8e", "warn" => "#f5a524", _ => "#ff5c6c" };
    format!(
        "<div style=\"font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;max-width:640px;margin:0 auto;padding:24px\">\
         <div style=\"border-left:4px solid {color};padding:8px 12px;background:#f6f7f9\"><h2 style=\"margin:0 0 4px\">{} {}</h2><div style=\"color:#555\">{}</div></div>\
         <p style=\"font-size:15px\">{}</p>{}{}<p style=\"color:#888;font-size:12px\">Galileo · {}</p></div>",
        emoji(n), html_escape(&n.title), n.state, html_escape(&n.message),
        n.value.map(|v| format!("<p>value <b>{v:.3}</b>{}</p>", n.threshold.map(|t| format!(" · threshold {t}")).unwrap_or_default())).unwrap_or_default(),
        n.url.as_ref().map(|u| format!("<p><a href=\"{u}\" style=\"background:#f5a524;color:#1a1200;padding:8px 14px;border-radius:6px;text-decoration:none\">Open in Galileo</a></p>")).unwrap_or_default(),
        n.at.format("%Y-%m-%d %H:%M UTC")
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub struct Mailer {
    pub cfg: SmtpConfig,
}

impl Mailer {
    pub fn enabled(&self) -> bool {
        !self.cfg.host.trim().is_empty()
    }

    fn transport(&self) -> anyhow::Result<AsyncSmtpTransport<Tokio1Executor>> {
        let mut b = match self.cfg.security.as_str() {
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.cfg.host),
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&self.cfg.host)?,
            _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.cfg.host)?,
        };
        b = b.port(self.cfg.port);
        if !self.cfg.user.is_empty() {
            b = b.credentials(Credentials::new(self.cfg.user.clone(), self.cfg.password.clone()));
        }
        Ok(b.build())
    }

    pub async fn send(&self, to: &[String], subject: &str, html: &str, text: &str) -> anyhow::Result<()> {
        if !self.enabled() { anyhow::bail!("SMTP is not configured"); }
        let from: Mailbox = self.cfg.from.parse()?;
        let mut m = Message::builder().from(from).subject(subject);
        for t in to { m = m.to(t.parse()?); }
        let msg = m.multipart(MultiPart::alternative().singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text.to_string())).singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html.to_string())))?;
        self.transport()?.send(msg).await?;
        Ok(())
    }
}

pub async fn send_target(http: &reqwest::Client, mailer: &Mailer, t: &Target, n: &Notification) -> anyhow::Result<()> {
    match t {
        Target::Webhook(url) => { http.post(url).json(n).send().await?.error_for_status()?; }
        Target::Slack(url) => { http.post(url).json(&slack_payload(n)).send().await?.error_for_status()?; }
        Target::Discord(url) => { http.post(url).json(&discord_payload(n)).send().await?.error_for_status()?; }
        Target::Telegram { bot_token, chat_id } => {
            http.post(format!("https://api.telegram.org/bot{bot_token}/sendMessage")).json(&json!({ "chat_id": chat_id, "text": telegram_text(n), "parse_mode": "HTML", "disable_web_page_preview": true })).send().await?.error_for_status()?;
        }
        Target::Email(to) => { mailer.send(to, &format!("[Galileo] {} — {}", n.title, n.state), &email_html(n), &format!("{}\n{}\n{}", n.title, n.message, n.url.clone().unwrap_or_default())).await?; }
    }
    Ok(())
}

pub async fn send_all(http: &reqwest::Client, mailer: &Mailer, targets: &[Target], n: &Notification) -> usize {
    let mut ok = 0;
    for t in targets {
        match send_target(http, mailer, t, n).await {
            Ok(()) => ok += 1,
            Err(e) => warn!(error = %e, ?t, "notification failed"),
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    fn n() -> Notification {
        Notification { kind: "trigger", state: "critical".into(), name: "x".into(), project_id: Uuid::nil(), title: "p95 <high>".into(), message: "over 800ms".into(), value: Some(1234.5), threshold: Some(800.0), url: Some("http://g/x".into()), at: chrono::Utc::now() }
    }
    #[test]
    fn payloads() {
        let s = slack_payload(&n());
        assert!(s["blocks"].as_array().unwrap().len() == 4);
        assert!(s["text"].as_str().unwrap().contains("🚨"));
        let d = discord_payload(&n());
        assert_eq!(d["embeds"][0]["color"], 0xff5c6c);
        assert!(telegram_text(&n()).contains("&lt;high&gt;"));
        assert!(email_html(&n()).contains("Open in Galileo"));
        assert!(matches!(target_from_channel("telegram", &json!({"bot_token":"t","chat_id":"c"})), Some(Target::Telegram { .. })));
        assert!(target_from_channel("email", &json!({"to": ["a@b.c"]})).is_some());
        assert!(target_from_channel("email", &json!({})).is_none());
    }
    #[test]
    fn recipients_parse_channel_refs() {
        let r = parse_recipients(&json!([{"type":"channel","id":"00000000-0000-0000-0000-000000000001"},{"type":"slack","url":"http://x"}]));
        assert_eq!(r.len(), 2);
    }
}
