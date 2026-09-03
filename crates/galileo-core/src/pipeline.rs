//! Log pipelines (per project, applied at ingest) and log-based metrics.

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{AttributeValue, LogRecord};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Match {
    pub field: String,
    #[serde(default = "d_op")]
    pub op: String,
    #[serde(default)]
    pub value: String,
}
fn d_op() -> String { "contains".into() }

impl Match {
    fn field_value(&self, l: &LogRecord) -> Option<String> {
        match self.field.as_str() {
            "body" => Some(l.body.clone()),
            "severity" => Some(format!("{:?}", l.severity).to_lowercase()),
            "service_name" | "service.name" => Some(l.service_name.clone()),
            f => l.attributes.get(f).or_else(|| l.resource.get(f)).map(attr_str),
        }
    }
    pub fn matches(&self, l: &LogRecord) -> bool {
        let Some(v) = self.field_value(l) else { return self.op == "not_exists" };
        match self.op.as_str() {
            "exists" => true,
            "not_exists" => false,
            "eq" | "=" => v == self.value,
            "ne" | "!=" => v != self.value,
            "starts_with" => v.starts_with(&self.value),
            "regex" | "~" => Regex::new(&self.value).map(|r| r.is_match(&v)).unwrap_or(false),
            _ => v.to_lowercase().contains(&self.value.to_lowercase()),
        }
    }
}

pub fn attr_str(v: &AttributeValue) -> String {
    match serde_json::to_value(v) { Ok(serde_json::Value::String(s)) => s, Ok(o) => o.to_string(), Err(_) => String::new() }
}

/// One step of a pipeline. `when` (optional) gates the step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Processor {
    /// Parse a JSON body into attributes (top-level keys; nested objects become dotted keys). Keeps
    /// `message`/`msg`/`event` as the body when present.
    JsonParse { #[serde(default)] when: Option<Match>, #[serde(default)] keep_body: bool },
    /// Named capture groups of `pattern` on `field` (default body) become attributes.
    RegexExtract { #[serde(default)] when: Option<Match>, pattern: String, #[serde(default = "d_body")] field: String },
    Rename { #[serde(default)] when: Option<Match>, from: String, to: String },
    /// Drop the record when `when` matches.
    Drop { when: Match },
    /// Set severity from a regex on the body (first match wins): `[{ pattern, severity }]`.
    SeverityMap { #[serde(default)] when: Option<Match>, rules: Vec<SeverityRule> },
    AddField { #[serde(default)] when: Option<Match>, key: String, value: String },
}
fn d_body() -> String { "body".into() }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeverityRule { pub pattern: String, pub severity: String }

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LogPipeline {
    #[serde(default)]
    pub processors: Vec<Processor>,
    #[serde(default = "d_true")]
    pub enabled: bool,
}
fn d_true() -> bool { true }

fn flatten(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, AttributeValue)>) {
    match v {
        serde_json::Value::Object(m) => for (k, x) in m { let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }; flatten(&key, x, out); },
        serde_json::Value::String(s) => out.push((prefix.to_string(), AttributeValue::Str(s.clone()))),
        serde_json::Value::Bool(b) => out.push((prefix.to_string(), AttributeValue::Bool(*b))),
        serde_json::Value::Number(n) => out.push((prefix.to_string(), if let Some(i) = n.as_i64() { AttributeValue::Int(i) } else { AttributeValue::Float(n.as_f64().unwrap_or(0.0)) })),
        serde_json::Value::Array(a) => out.push((prefix.to_string(), AttributeValue::Str(serde_json::to_string(a).unwrap_or_default()))),
        serde_json::Value::Null => {}
    }
}

impl LogPipeline {
    /// Apply every processor in order. Returns false when the record must be dropped.
    pub fn apply(&self, l: &mut LogRecord) -> bool {
        if !self.enabled { return true; }
        for p in &self.processors {
            match p {
                Processor::JsonParse { when, keep_body } => {
                    if when.as_ref().map(|w| !w.matches(l)).unwrap_or(false) { continue; }
                    let t = l.body.trim();
                    if !(t.starts_with('{') && t.ends_with('}')) { continue; }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(t) else { continue };
                    let mut out = vec![];
                    flatten("", &v, &mut out);
                    let mut new_body = None;
                    for (k, val) in out {
                        if matches!(k.as_str(), "message" | "msg" | "event") && new_body.is_none() { new_body = Some(attr_str(&val)); }
                        l.attributes.insert(k, val);
                    }
                    if !keep_body { if let Some(b) = new_body { l.body = b; } }
                }
                Processor::RegexExtract { when, pattern, field } => {
                    if when.as_ref().map(|w| !w.matches(l)).unwrap_or(false) { continue; }
                    let Ok(re) = Regex::new(pattern) else { continue };
                    let hay = if field == "body" { l.body.clone() } else { l.attributes.get(field).map(attr_str).unwrap_or_default() };
                    if let Some(c) = re.captures(&hay) {
                        for name in re.capture_names().flatten() {
                            if let Some(m) = c.name(name) { l.attributes.insert(name.to_string(), AttributeValue::Str(m.as_str().to_string())); }
                        }
                    }
                }
                Processor::Rename { when, from, to } => {
                    if when.as_ref().map(|w| !w.matches(l)).unwrap_or(false) { continue; }
                    if let Some(v) = l.attributes.shift_remove(from) { l.attributes.insert(to.clone(), v); }
                }
                Processor::Drop { when } => { if when.matches(l) { return false; } }
                Processor::SeverityMap { when, rules } => {
                    if when.as_ref().map(|w| !w.matches(l)).unwrap_or(false) { continue; }
                    for r in rules {
                        if Regex::new(&r.pattern).map(|re| re.is_match(&l.body)).unwrap_or(false) {
                            l.severity = crate::log::Severity::from_text(&r.severity);
                            l.severity_text = r.severity.to_uppercase();
                            break;
                        }
                    }
                }
                Processor::AddField { when, key, value } => {
                    if when.as_ref().map(|w| !w.matches(l)).unwrap_or(false) { continue; }
                    l.attributes.insert(key.clone(), AttributeValue::Str(value.clone()));
                }
            }
        }
        true
    }
}

/// A metric derived from logs at ingest: count of matching records, or the numeric value of an
/// attribute on matching records.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogMetric {
    pub name: String,
    #[serde(rename = "match")]
    pub matcher: Match,
    /// Attribute whose numeric value becomes the point value; absent = count (1 per record).
    #[serde(default)]
    pub value_from: Option<String>,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "d_true")]
    pub enabled: bool,
}

impl LogMetric {
    pub fn value(&self, l: &LogRecord) -> Option<f64> {
        if !self.enabled || !self.matcher.matches(l) { return None; }
        match &self.value_from {
            None => Some(1.0),
            Some(k) => l.attributes.get(k).and_then(|v| match v { AttributeValue::Int(i) => Some(*i as f64), AttributeValue::Float(f) => Some(*f), AttributeValue::Str(s) => s.trim().parse().ok(), _ => None }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attributes, ProjectId};
    use chrono::Utc;

    fn rec(body: &str) -> LogRecord {
        LogRecord { project_id: ProjectId(Default::default()), timestamp: Utc::now(), observed_timestamp: Utc::now(), severity: crate::log::Severity::Info, severity_text: "INFO".into(), body: body.into(), body_value: None, trace_id: None, span_id: None, service_name: "svc".into(), scope_name: String::new(), resource: Attributes::default(), attributes: Attributes::default() }
    }

    #[test]
    fn json_parse_and_extract() {
        let p = LogPipeline { enabled: true, processors: vec![
            Processor::JsonParse { when: None, keep_body: false },
            Processor::RegexExtract { when: None, pattern: r"user=(?P<user>\w+)".into(), field: "body".into() },
            Processor::Rename { when: None, from: "req".into(), to: "request.id".into() },
            Processor::SeverityMap { when: None, rules: vec![SeverityRule { pattern: "(?i)failed".into(), severity: "error".into() }] },
            Processor::AddField { when: None, key: "pipeline".into(), value: "v1".into() },
        ] };
        let mut l = rec(r#"{"msg": "login failed user=ana", "req": "r1", "ctx": {"tenant": "acme"}}"#);
        assert!(p.apply(&mut l));
        assert_eq!(l.body, "login failed user=ana");
        assert_eq!(attr_str(l.attributes.get("user").unwrap()), "ana");
        assert_eq!(attr_str(l.attributes.get("request.id").unwrap()), "r1");
        assert_eq!(attr_str(l.attributes.get("ctx.tenant").unwrap()), "acme");
        assert_eq!(l.severity_text, "ERROR");
        assert!(l.attributes.get("pipeline").is_some());
    }
    #[test]
    fn drop_and_metrics() {
        let p = LogPipeline { enabled: true, processors: vec![Processor::Drop { when: Match { field: "body".into(), op: "contains".into(), value: "healthz".into() } }] };
        assert!(!p.apply(&mut rec("GET /healthz 200")));
        assert!(p.apply(&mut rec("GET /orders 200")));
        let m = LogMetric { name: "login_failures".into(), matcher: Match { field: "body".into(), op: "regex".into(), value: "login failed".into() }, value_from: None, unit: "1".into(), enabled: true };
        assert_eq!(m.value(&rec("login failed user=x")), Some(1.0));
        assert_eq!(m.value(&rec("ok")), None);
        let mut l = rec("slow"); l.attributes.insert("elapsed_ms".into(), AttributeValue::Float(42.5));
        let m2 = LogMetric { name: "elapsed".into(), matcher: Match { field: "elapsed_ms".into(), op: "exists".into(), value: String::new() }, value_from: Some("elapsed_ms".into()), unit: "ms".into(), enabled: true };
        assert_eq!(m2.value(&l), Some(42.5));
    }
}

/// Ingest quotas per project and day.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Quotas {
    #[serde(default)] pub spans_per_day: Option<u64>,
    #[serde(default)] pub logs_per_day: Option<u64>,
    #[serde(default)] pub metrics_per_day: Option<u64>,
    /// "warn" (notify, keep ingesting) or "hard" (429 once exceeded).
    #[serde(default = "d_mode")] pub mode: String,
}
fn d_mode() -> String { "warn".into() }
impl Quotas {
    pub fn is_empty(&self) -> bool { self.spans_per_day.is_none() && self.logs_per_day.is_none() && self.metrics_per_day.is_none() }
    pub fn limit(&self, signal: &str) -> Option<u64> { match signal { "spans" => self.spans_per_day, "logs" => self.logs_per_day, _ => self.metrics_per_day } }
}
