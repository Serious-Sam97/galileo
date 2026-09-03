//! The two wire formats the gateway speaks, a normalized text-only model used when translating
//! between them, and streaming observers/translators over server-sent events.
//!
//! Passthrough (client format == provider format) keeps every field intact, including tools;
//! translation is text-only, which covers chat and completion-style usage across providers.

use bytes::Bytes;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Anthropic,
    Openai,
}

impl Format {
    pub fn as_str(&self) -> &'static str {
        match self {
            Format::Anthropic => "anthropic",
            Format::Openai => "openai",
        }
    }
}

/// Body cap for prompt/completion text recorded on spans.
pub const TEXT_CAP: usize = 8 * 1024;

pub fn cap(s: &str) -> String {
    if s.len() <= TEXT_CAP {
        s.to_string()
    } else {
        let mut end = TEXT_CAP;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…[truncated {} bytes]", &s[..end], s.len() - end)
    }
}

// ------------------------------------------------------------------------------------------
// normalized request / response
// ------------------------------------------------------------------------------------------

/// One piece of a message. Text, images, tool calls and tool results all round-trip between
/// the Anthropic and OpenAI shapes; anything else degrades to text.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    /// base64 data or a URL, with a media type when known.
    Image { media_type: String, data: Option<String>, url: Option<String> },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormMessage {
    pub role: String,
    pub parts: Vec<Part>,
}

impl NormMessage {
    pub fn text(role: &str, content: &str) -> Self {
        Self { role: role.into(), parts: vec![Part::Text(content.into())] }
    }
    /// Flattened text (what the span records and what the tests compare).
    pub fn content(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                Part::Text(t) => t.clone(),
                Part::Image { .. } => "[image]".into(),
                Part::ToolUse { name, input, .. } => format!("[tool_use {name}: {input}]"),
                Part::ToolResult { content, .. } => format!("[tool_result] {content}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ToolChoice {
    #[default]
    None,
    Auto,
    Required,
    Named(String),
}

#[derive(Debug, Clone, Default)]
pub struct NormRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<NormMessage>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop: Vec<String>,
    pub stream: bool,
    pub tools: Vec<ToolDef>,
    pub tool_choice: ToolChoice,
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(str::to_owned)).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Anthropic content (string or blocks) → parts.
fn anthropic_parts(v: &Value) -> Vec<Part> {
    match v {
        Value::String(s) => vec![Part::Text(s.clone())],
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| match b.get("type").and_then(|t| t.as_str()).unwrap_or("text") {
                "text" => b.get("text").and_then(|t| t.as_str()).map(|t| Part::Text(t.to_string())),
                "image" => {
                    let src = b.get("source")?;
                    Some(Part::Image {
                        media_type: src.get("media_type").and_then(|m| m.as_str()).unwrap_or("image/png").to_string(),
                        data: src.get("data").and_then(|d| d.as_str()).map(str::to_owned),
                        url: src.get("url").and_then(|d| d.as_str()).map(str::to_owned),
                    })
                }
                "tool_use" => Some(Part::ToolUse {
                    id: b.get("id").and_then(|i| i.as_str()).unwrap_or("").into(),
                    name: b.get("name").and_then(|n| n.as_str()).unwrap_or("").into(),
                    input: b.get("input").cloned().unwrap_or(json!({})),
                }),
                "tool_result" => Some(Part::ToolResult {
                    tool_use_id: b.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("").into(),
                    content: b.get("content").map(text_of).unwrap_or_default(),
                    is_error: b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false),
                }),
                "document" => Some(Part::Text("[document]".into())),
                _ => None,
            })
            .collect(),
        Value::Null => vec![],
        other => vec![Part::Text(other.to_string())],
    }
}

/// OpenAI message content (string or parts) → parts (text + images).
fn openai_parts(v: &Value) -> Vec<Part> {
    match v {
        Value::String(s) => vec![Part::Text(s.clone())],
        Value::Array(items) => items
            .iter()
            .filter_map(|b| match b.get("type").and_then(|t| t.as_str()).unwrap_or("text") {
                "text" | "input_text" => b.get("text").and_then(|t| t.as_str()).map(|t| Part::Text(t.to_string())),
                "image_url" => {
                    let url = b.get("image_url").and_then(|u| u.get("url")).and_then(|u| u.as_str()).unwrap_or("");
                    Some(match parse_data_url(url) {
                        Some((mt, data)) => Part::Image { media_type: mt, data: Some(data), url: None },
                        None => Part::Image { media_type: "image/jpeg".into(), data: None, url: Some(url.to_string()) },
                    })
                }
                _ => None,
            })
            .collect(),
        Value::Null => vec![],
        other => vec![Part::Text(other.to_string())],
    }
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mt = meta.split(';').next().unwrap_or("image/png").to_string();
    Some((mt, data.to_string()))
}

pub fn parse_request(format: Format, body: &Value) -> NormRequest {
    let model = body.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string();
    let stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let temperature = body.get("temperature").and_then(|t| t.as_f64());
    let top_p = body.get("top_p").and_then(|t| t.as_f64());
    let mut out = NormRequest { model, stream, temperature, top_p, ..Default::default() };
    match format {
        Format::Anthropic => {
            out.max_tokens = body.get("max_tokens").and_then(|m| m.as_u64());
            out.system = body.get("system").map(text_of).filter(|s| !s.is_empty());
            out.stop = body.get("stop_sequences").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            for m in body.get("messages").and_then(|m| m.as_array()).into_iter().flatten() {
                out.messages.push(NormMessage {
                    role: m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string(),
                    parts: m.get("content").map(anthropic_parts).unwrap_or_default(),
                });
            }
            for t in body.get("tools").and_then(|t| t.as_array()).into_iter().flatten() {
                if let Some(name) = t.get("name").and_then(|n| n.as_str()) {
                    out.tools.push(ToolDef { name: name.into(), description: t.get("description").and_then(|d| d.as_str()).unwrap_or("").into(), schema: t.get("input_schema").cloned().unwrap_or(json!({"type":"object","properties":{}})) });
                }
            }
            out.tool_choice = match body.get("tool_choice").and_then(|c| c.get("type")).and_then(|t| t.as_str()) {
                Some("any") => ToolChoice::Required,
                Some("tool") => ToolChoice::Named(body["tool_choice"]["name"].as_str().unwrap_or("").into()),
                Some("none") => ToolChoice::None,
                Some("auto") => ToolChoice::Auto,
                _ => if out.tools.is_empty() { ToolChoice::None } else { ToolChoice::Auto },
            };
        }
        Format::Openai => {
            out.max_tokens = body.get("max_completion_tokens").or_else(|| body.get("max_tokens")).and_then(|m| m.as_u64());
            out.stop = match body.get("stop") {
                Some(Value::String(s)) => vec![s.clone()],
                Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
                _ => vec![],
            };
            let mut systems = Vec::new();
            for m in body.get("messages").and_then(|m| m.as_array()).into_iter().flatten() {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                match role {
                    "system" | "developer" => systems.push(text_of(m.get("content").unwrap_or(&Value::Null))),
                    "tool" => out.messages.push(NormMessage { role: "user".into(), parts: vec![Part::ToolResult { tool_use_id: m.get("tool_call_id").and_then(|i| i.as_str()).unwrap_or("").into(), content: text_of(m.get("content").unwrap_or(&Value::Null)), is_error: false }] }),
                    "assistant" => {
                        let mut parts = openai_parts(m.get("content").unwrap_or(&Value::Null));
                        for tc in m.get("tool_calls").and_then(|t| t.as_array()).into_iter().flatten() {
                            let f = tc.get("function").cloned().unwrap_or(json!({}));
                            let args = f.get("arguments").and_then(|a| a.as_str()).map(|a| serde_json::from_str::<Value>(a).unwrap_or(json!({"_raw": a}))).unwrap_or(json!({}));
                            parts.push(Part::ToolUse { id: tc.get("id").and_then(|i| i.as_str()).unwrap_or("").into(), name: f.get("name").and_then(|n| n.as_str()).unwrap_or("").into(), input: args });
                        }
                        out.messages.push(NormMessage { role: "assistant".into(), parts });
                    }
                    _ => out.messages.push(NormMessage { role: "user".into(), parts: openai_parts(m.get("content").unwrap_or(&Value::Null)) }),
                }
            }
            if !systems.is_empty() {
                out.system = Some(systems.join("\n\n"));
            }
            for t in body.get("tools").and_then(|t| t.as_array()).into_iter().flatten() {
                if let Some(f) = t.get("function") {
                    if let Some(name) = f.get("name").and_then(|n| n.as_str()) {
                        out.tools.push(ToolDef { name: name.into(), description: f.get("description").and_then(|d| d.as_str()).unwrap_or("").into(), schema: f.get("parameters").cloned().unwrap_or(json!({"type":"object","properties":{}})) });
                    }
                }
            }
            out.tool_choice = match body.get("tool_choice") {
                Some(Value::String(s)) if s == "required" => ToolChoice::Required,
                Some(Value::String(s)) if s == "none" => ToolChoice::None,
                Some(Value::String(_)) => ToolChoice::Auto,
                Some(obj) if obj.is_object() => ToolChoice::Named(obj["function"]["name"].as_str().unwrap_or("").into()),
                _ => if out.tools.is_empty() { ToolChoice::None } else { ToolChoice::Auto },
            };
        }
    }
    // Anthropic requires alternating roles starting with user; merge adjacent same-role turns.
    let mut merged: Vec<NormMessage> = Vec::with_capacity(out.messages.len());
    for m in out.messages.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.role == m.role {
                last.parts.extend(m.parts);
                continue;
            }
        }
        merged.push(m);
    }
    if merged.first().map(|m| m.role == "assistant").unwrap_or(false) {
        merged.insert(0, NormMessage::text("user", "(continue)"));
    }
    out.messages = merged;
    out
}

fn parts_to_anthropic(parts: &[Part]) -> Value {
    if parts.len() == 1 {
        if let Part::Text(t) = &parts[0] {
            return json!(t);
        }
    }
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                Part::Text(t) => json!({ "type": "text", "text": t }),
                Part::Image { media_type, data: Some(d), .. } => json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": d } }),
                Part::Image { url: Some(u), .. } => json!({ "type": "image", "source": { "type": "url", "url": u } }),
                Part::Image { .. } => json!({ "type": "text", "text": "[image]" }),
                Part::ToolUse { id, name, input } => json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
                Part::ToolResult { tool_use_id, content, is_error } => json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error }),
            })
            .collect(),
    )
}

/// One normalized message → one or more OpenAI messages (tool results become `tool` messages).
fn message_to_openai(m: &NormMessage) -> Vec<Value> {
    let mut out = Vec::new();
    let mut content_parts: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut only_text = true;
    for p in &m.parts {
        match p {
            Part::Text(t) => content_parts.push(json!({ "type": "text", "text": t })),
            Part::Image { media_type, data: Some(d), .. } => { only_text = false; content_parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{media_type};base64,{d}") } })); }
            Part::Image { url: Some(u), .. } => { only_text = false; content_parts.push(json!({ "type": "image_url", "image_url": { "url": u } })); }
            Part::Image { .. } => content_parts.push(json!({ "type": "text", "text": "[image]" })),
            Part::ToolUse { id, name, input } => tool_calls.push(json!({ "id": id, "type": "function", "function": { "name": name, "arguments": input.to_string() } })),
            Part::ToolResult { tool_use_id, content, .. } => out.push(json!({ "role": "tool", "tool_call_id": tool_use_id, "content": content })),
        }
    }
    if !content_parts.is_empty() || !tool_calls.is_empty() {
        let content = if only_text {
            let t = content_parts.iter().filter_map(|c| c.get("text").and_then(|x| x.as_str())).collect::<Vec<_>>().join("\n");
            if t.is_empty() && !tool_calls.is_empty() { Value::Null } else { json!(t) }
        } else {
            Value::Array(content_parts)
        };
        let mut msg = json!({ "role": m.role, "content": content });
        if !tool_calls.is_empty() {
            msg["tool_calls"] = Value::Array(tool_calls);
        }
        // tool results must come after the assistant message that called them; parts keep order
        out.insert(0, msg);
    }
    out
}

pub fn build_request(format: Format, req: &NormRequest, model: &str, openai_strict: bool) -> Value {
    match format {
        Format::Anthropic => {
            let mut b = json!({
                "model": model,
                "max_tokens": req.max_tokens.unwrap_or(4096),
                "messages": req.messages.iter().map(|m| json!({ "role": m.role, "content": parts_to_anthropic(&m.parts) })).collect::<Vec<_>>(),
                "stream": req.stream,
            });
            if let Some(s) = &req.system { b["system"] = json!(s); }
            if let Some(t) = req.temperature { b["temperature"] = json!(t); }
            if let Some(t) = req.top_p { b["top_p"] = json!(t); }
            if !req.stop.is_empty() { b["stop_sequences"] = json!(req.stop); }
            if !req.tools.is_empty() {
                b["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema })).collect());
                b["tool_choice"] = match &req.tool_choice {
                    ToolChoice::Required => json!({ "type": "any" }),
                    ToolChoice::Named(n) => json!({ "type": "tool", "name": n }),
                    ToolChoice::None => json!({ "type": "none" }),
                    ToolChoice::Auto => json!({ "type": "auto" }),
                };
            }
            b
        }
        Format::Openai => {
            let mut msgs = Vec::new();
            if let Some(s) = &req.system { msgs.push(json!({ "role": "system", "content": s })); }
            for m in &req.messages { msgs.extend(message_to_openai(m)); }
            let mut b = json!({ "model": model, "messages": msgs, "stream": req.stream });
            if let Some(mt) = req.max_tokens { b[if openai_strict { "max_completion_tokens" } else { "max_tokens" }] = json!(mt); }
            if let Some(t) = req.temperature { b["temperature"] = json!(t); }
            if let Some(t) = req.top_p { b["top_p"] = json!(t); }
            if !req.stop.is_empty() { b["stop"] = json!(req.stop); }
            if req.stream { b["stream_options"] = json!({ "include_usage": true }); }
            if !req.tools.is_empty() {
                b["tools"] = Value::Array(req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } })).collect());
                b["tool_choice"] = match &req.tool_choice {
                    ToolChoice::Required => json!("required"),
                    ToolChoice::Named(n) => json!({ "type": "function", "function": { "name": n } }),
                    ToolChoice::None => json!("none"),
                    ToolChoice::Auto => json!("auto"),
                };
            }
            b
        }
    }
}

/// Prompt text as recorded on the span: system + turns, in a readable form.
pub fn prompt_text(req: &NormRequest) -> String {
    let mut s = String::new();
    if let Some(sys) = &req.system {
        s.push_str("[system] ");
        s.push_str(sys);
        s.push('\n');
    }
    for m in &req.messages {
        s.push('[');
        s.push_str(&m.role);
        s.push_str("] ");
        s.push_str(&m.content());
        s.push('\n');
    }
    s
}

/// What we learn from a response regardless of format. Filled by `observe_*` for passthrough
/// and by the translators.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    pub id: String,
    pub model: String,
    pub text: String,
    pub stop_reason: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub saw_usage: bool,
    /// Tool calls made by the model (id, name, JSON arguments).
    pub tool_calls: Vec<(String, String, Value)>,
}

impl Observed {
    fn push_text(&mut self, t: &str) {
        if self.text.len() < TEXT_CAP * 2 {
            self.text.push_str(t);
        }
    }
}

fn u(v: Option<&Value>) -> u64 {
    v.and_then(|x| x.as_u64()).unwrap_or(0)
}

pub fn observe_response(format: Format, body: &Value) -> Observed {
    let mut o = Observed {
        id: body.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        model: body.get("model").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        ..Default::default()
    };
    match format {
        Format::Anthropic => {
            o.stop_reason = body.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("").to_string();
            for b in body.get("content").and_then(|c| c.as_array()).into_iter().flatten() {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => o.push_text(b.get("text").and_then(|t| t.as_str()).unwrap_or("")),
                    Some("tool_use") => o.tool_calls.push((b.get("id").and_then(|i| i.as_str()).unwrap_or("").into(), b.get("name").and_then(|n| n.as_str()).unwrap_or("").into(), b.get("input").cloned().unwrap_or(json!({})))),
                    _ => {}
                }
            }
            if let Some(us) = body.get("usage") {
                o.saw_usage = true;
                o.input_tokens = u(us.get("input_tokens"));
                o.output_tokens = u(us.get("output_tokens"));
                o.cache_read_tokens = u(us.get("cache_read_input_tokens"));
                o.cache_write_tokens = u(us.get("cache_creation_input_tokens"));
            }
        }
        Format::Openai => {
            if let Some(c) = body.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first()) {
                o.stop_reason = c.get("finish_reason").and_then(|s| s.as_str()).unwrap_or("").to_string();
                if let Some(t) = c.get("message").and_then(|m| m.get("content")) {
                    o.push_text(&text_of(t));
                }
                for tc in c.get("message").and_then(|m| m.get("tool_calls")).and_then(|t| t.as_array()).into_iter().flatten() {
                    let f = tc.get("function").cloned().unwrap_or(json!({}));
                    let args = f.get("arguments").and_then(|a| a.as_str()).map(|a| serde_json::from_str::<Value>(a).unwrap_or(json!({"_raw": a}))).unwrap_or(json!({}));
                    o.tool_calls.push((tc.get("id").and_then(|i| i.as_str()).unwrap_or("").into(), f.get("name").and_then(|n| n.as_str()).unwrap_or("").into(), args));
                }
            }
            if let Some(us) = body.get("usage") {
                o.saw_usage = true;
                o.input_tokens = u(us.get("prompt_tokens"));
                o.output_tokens = u(us.get("completion_tokens"));
                o.cache_read_tokens = u(us.get("prompt_tokens_details").and_then(|d| d.get("cached_tokens")));
            }
        }
    }
    o
}

pub fn map_stop(from: Format, to: Format, reason: &str) -> String {
    if from == to || reason.is_empty() {
        return reason.to_string();
    }
    match (to, reason) {
        (Format::Openai, "end_turn") | (Format::Openai, "stop_sequence") => "stop".into(),
        (Format::Openai, "max_tokens") => "length".into(),
        (Format::Openai, "tool_use") => "tool_calls".into(),
        (Format::Openai, "refusal") => "content_filter".into(),
        (Format::Anthropic, "stop") => "end_turn".into(),
        (Format::Anthropic, "length") => "max_tokens".into(),
        (Format::Anthropic, "tool_calls") => "tool_use".into(),
        (Format::Anthropic, "content_filter") => "refusal".into(),
        (_, r) => r.into(),
    }
}

/// A complete non-streaming response in `format` built from an observation (used after
/// translating a non-streaming call).
pub fn build_response(format: Format, o: &Observed, from: Format) -> Value {
    let mut stop = map_stop(from, format, &o.stop_reason);
    if !o.tool_calls.is_empty() {
        stop = if format == Format::Anthropic { "tool_use".into() } else { "tool_calls".into() };
    }
    match format {
        Format::Anthropic => {
            let mut content: Vec<Value> = Vec::new();
            if !o.text.is_empty() || o.tool_calls.is_empty() { content.push(json!({ "type": "text", "text": o.text })); }
            for (id, name, input) in &o.tool_calls {
                content.push(json!({ "type": "tool_use", "id": if id.is_empty() { format!("toolu_{}", uuid::Uuid::new_v4().simple()) } else { id.clone() }, "name": name, "input": input }));
            }
            json!({
            "id": if o.id.is_empty() { format!("msg_{}", uuid::Uuid::new_v4().simple()) } else { o.id.clone() },
            "type": "message",
            "role": "assistant",
            "model": o.model,
            "content": content,
            "stop_reason": if stop.is_empty() { "end_turn".to_string() } else { stop },
            "stop_sequence": null,
            "usage": { "input_tokens": o.input_tokens, "output_tokens": o.output_tokens,
                       "cache_read_input_tokens": o.cache_read_tokens, "cache_creation_input_tokens": o.cache_write_tokens }
        })
        }
        Format::Openai => {
            let mut message = json!({ "role": "assistant", "content": if o.text.is_empty() && !o.tool_calls.is_empty() { Value::Null } else { json!(o.text) } });
            if !o.tool_calls.is_empty() {
                message["tool_calls"] = Value::Array(o.tool_calls.iter().map(|(id, name, input)| json!({ "id": if id.is_empty() { format!("call_{}", uuid::Uuid::new_v4().simple()) } else { id.clone() }, "type": "function", "function": { "name": name, "arguments": input.to_string() } })).collect());
            }
            json!({
            "id": if o.id.is_empty() { format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()) } else { o.id.clone() },
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": o.model,
            "choices": [{ "index": 0, "message": message, "finish_reason": if stop.is_empty() { "stop".to_string() } else { stop } }],
            "usage": { "prompt_tokens": o.input_tokens, "completion_tokens": o.output_tokens, "total_tokens": o.input_tokens + o.output_tokens }
        })
        }
    }
}

/// Error body in the client's format.
pub fn error_body(format: Format, status: u16, kind: &str, message: &str) -> Value {
    match format {
        Format::Anthropic => json!({ "type": "error", "error": { "type": kind, "message": message } }),
        Format::Openai => json!({ "error": { "message": message, "type": kind, "code": status } }),
    }
}

// ------------------------------------------------------------------------------------------
// server-sent events
// ------------------------------------------------------------------------------------------

/// Incremental SSE parser: feed bytes, get `(event, data)` pairs.
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((frame_len, sep_len)) = find_double_newline(&self.buf) {
            let frame = String::from_utf8_lossy(&self.buf[..frame_len]).into_owned();
            self.buf.drain(..frame_len + sep_len);
            let mut event = String::new();
            let mut data = Vec::new();
            for line in frame.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
                }
            }
            if !data.is_empty() {
                out.push((event, data.join("\n")));
            }
        }
        out
    }
}

fn find_double_newline(b: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\n' && b[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < b.len() && &b[i..i + 4] == b"\r\n\r\n" {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

pub fn frame(format: Format, event: &str, data: &Value) -> Bytes {
    match format {
        Format::Anthropic => Bytes::from(format!("event: {event}\ndata: {data}\n\n")),
        Format::Openai => Bytes::from(format!("data: {data}\n\n")),
    }
}

pub fn done_frame() -> Bytes {
    Bytes::from_static(b"data: [DONE]\n\n")
}

/// Watches a passthrough stream in `format` and fills an `Observed`.
pub struct StreamObserver {
    pub format: Format,
    pub observed: Observed,
    pub first_token_at: Option<std::time::Instant>,
}

impl StreamObserver {
    pub fn new(format: Format) -> Self {
        Self { format, observed: Observed::default(), first_token_at: None }
    }

    pub fn feed(&mut self, data: &str) {
        if data.trim() == "[DONE]" {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { return };
        match self.format {
            Format::Anthropic => match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "message_start" => {
                    if let Some(m) = v.get("message") {
                        self.observed.id = m.get("id").and_then(|s| s.as_str()).unwrap_or("").into();
                        self.observed.model = m.get("model").and_then(|s| s.as_str()).unwrap_or("").into();
                        if let Some(us) = m.get("usage") {
                            self.observed.saw_usage = true;
                            self.observed.input_tokens = u(us.get("input_tokens"));
                            self.observed.cache_read_tokens = u(us.get("cache_read_input_tokens"));
                            self.observed.cache_write_tokens = u(us.get("cache_creation_input_tokens"));
                        }
                    }
                }
                "content_block_delta" => {
                    if let Some(t) = v.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                        self.first_token_at.get_or_insert_with(std::time::Instant::now);
                        self.observed.push_text(t);
                    }
                }
                "message_delta" => {
                    if let Some(s) = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|s| s.as_str()) {
                        self.observed.stop_reason = s.into();
                    }
                    if let Some(us) = v.get("usage") {
                        self.observed.saw_usage = true;
                        self.observed.output_tokens = u(us.get("output_tokens")).max(self.observed.output_tokens);
                        if let Some(i) = us.get("input_tokens").and_then(|x| x.as_u64()) {
                            self.observed.input_tokens = i;
                        }
                    }
                }
                _ => {}
            },
            Format::Openai => {
                if self.observed.id.is_empty() {
                    self.observed.id = v.get("id").and_then(|s| s.as_str()).unwrap_or("").into();
                }
                if self.observed.model.is_empty() {
                    self.observed.model = v.get("model").and_then(|s| s.as_str()).unwrap_or("").into();
                }
                if let Some(c) = v.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first()) {
                    if let Some(t) = c.get("delta").and_then(|d| d.get("content")).and_then(|t| t.as_str()) {
                        if !t.is_empty() {
                            self.first_token_at.get_or_insert_with(std::time::Instant::now);
                        }
                        self.observed.push_text(t);
                    }
                    if let Some(f) = c.get("finish_reason").and_then(|f| f.as_str()) {
                        self.observed.stop_reason = f.into();
                    }
                }
                if let Some(us) = v.get("usage").filter(|u| !u.is_null()) {
                    self.observed.saw_usage = true;
                    self.observed.input_tokens = u(us.get("prompt_tokens"));
                    self.observed.output_tokens = u(us.get("completion_tokens"));
                    self.observed.cache_read_tokens = u(us.get("prompt_tokens_details").and_then(|d| d.get("cached_tokens")));
                }
            }
        }
    }
}

/// Translates a stream from the provider's format into the client's, observing as it goes.
pub struct StreamTranslator {
    pub from: Format,
    pub to: Format,
    pub obs: StreamObserver,
    started: bool,
    block_open: bool,
    finished: bool,
    pending_finish: Option<String>,
    id: String,
    model: String,
    /// Tool calls in flight: (id, name, accumulated JSON arguments), by index.
    tools: Vec<(String, String, String)>,
    /// Anthropic → OpenAI: content block index → tool index.
    block_tool: std::collections::HashMap<u64, usize>,
    /// OpenAI → Anthropic: next Anthropic content block index to allocate.
    next_block: u64,
}

impl StreamTranslator {
    pub fn new(from: Format, to: Format, model_hint: &str) -> Self {
        Self {
            from,
            to,
            obs: StreamObserver::new(from),
            started: false,
            block_open: false,
            finished: false,
            pending_finish: None,
            id: String::new(),
            model: model_hint.to_string(),
            tools: Vec::new(),
            block_tool: std::collections::HashMap::new(),
            next_block: 1,
        }
    }

    fn tool_chunk(&self, index: usize, id: Option<&str>, name: Option<&str>, args: &str) -> Value {
        let mut tc = json!({ "index": index, "type": "function", "function": { "arguments": args } });
        if let Some(i) = id { tc["id"] = json!(i); }
        if let Some(n) = name { tc["function"]["name"] = json!(n); }
        self.chunk(json!({ "tool_calls": [tc] }), None)
    }

    fn ensure_started(&mut self, out: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        match self.to {
            Format::Anthropic => {
                let id = if self.id.is_empty() { format!("msg_{}", uuid::Uuid::new_v4().simple()) } else { self.id.clone() };
                self.id = id.clone();
                out.push(frame(
                    Format::Anthropic,
                    "message_start",
                    &json!({ "type": "message_start", "message": { "id": id, "type": "message", "role": "assistant", "model": self.model,
                        "content": [], "stop_reason": null, "stop_sequence": null,
                        "usage": { "input_tokens": self.obs.observed.input_tokens, "output_tokens": 0 } } }),
                ));
                out.push(frame(Format::Anthropic, "content_block_start", &json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } })));
                self.block_open = true;
            }
            Format::Openai => {
                let id = if self.id.is_empty() { format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()) } else { self.id.clone() };
                self.id = id.clone();
                out.push(frame(Format::Openai, "", &self.chunk(json!({ "role": "assistant", "content": "" }), None)));
            }
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> Value {
        json!({ "id": self.id, "object": "chat.completion.chunk", "created": chrono::Utc::now().timestamp(), "model": self.model,
                "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] })
    }

    pub fn feed(&mut self, data: &str) -> Vec<Bytes> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        if data.trim() == "[DONE]" {
            return self.finish();
        }
        self.obs.feed(data);
        let Ok(v) = serde_json::from_str::<Value>(data) else { return out };
        match self.from {
            Format::Openai => {
                if self.id.is_empty() {
                    if let Some(id) = v.get("id").and_then(|s| s.as_str()) {
                        self.id = id.to_string();
                    }
                }
                if let Some(m) = v.get("model").and_then(|s| s.as_str()) {
                    if !m.is_empty() {
                        self.model = m.to_string();
                    }
                }
                self.ensure_started(&mut out);
                if let Some(c) = v.get("choices").and_then(|c| c.as_array()).and_then(|a| a.first()) {
                    if let Some(t) = c.get("delta").and_then(|d| d.get("content")).and_then(|t| t.as_str()) {
                        if !t.is_empty() {
                            out.push(frame(Format::Anthropic, "content_block_delta", &json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": t } })));
                        }
                    }
                    for tc in c.get("delta").and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()).into_iter().flatten() {
                        let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                        while self.tools.len() <= idx { self.tools.push((String::new(), String::new(), String::new())); }
                        let block = (idx as u64) + 1;
                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) { self.tools[idx].0 = id.to_string(); }
                        if let Some(n) = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()) {
                            self.tools[idx].1 = n.to_string();
                            // first sight of this tool call: open its content block
                            out.push(frame(Format::Anthropic, "content_block_start", &json!({ "type": "content_block_start", "index": block, "content_block": { "type": "tool_use", "id": if self.tools[idx].0.is_empty() { format!("toolu_{idx}") } else { self.tools[idx].0.clone() }, "name": n, "input": {} } })));
                            self.next_block = self.next_block.max(block + 1);
                        }
                        if let Some(a) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()) {
                            if !a.is_empty() {
                                self.tools[idx].2.push_str(a);
                                out.push(frame(Format::Anthropic, "content_block_delta", &json!({ "type": "content_block_delta", "index": block, "delta": { "type": "input_json_delta", "partial_json": a } })));
                            }
                        }
                    }
                    if let Some(f) = c.get("finish_reason").and_then(|f| f.as_str()) {
                        self.pending_finish = Some(f.to_string());
                    }
                }
                // usage arrives in a trailing chunk (include_usage); finish once we have it, or at [DONE]
                if v.get("usage").map(|u| !u.is_null()).unwrap_or(false) && self.pending_finish.is_some() {
                    out.extend(self.finish());
                }
            }
            Format::Anthropic => {
                match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "message_start" => {
                        if let Some(m) = v.get("message") {
                            self.id = m.get("id").and_then(|s| s.as_str()).unwrap_or("").into();
                            if let Some(md) = m.get("model").and_then(|s| s.as_str()) {
                                self.model = md.into();
                            }
                        }
                        self.ensure_started(&mut out);
                    }
                    "content_block_start" => {
                        self.ensure_started(&mut out);
                        let cb = v.get("content_block").cloned().unwrap_or(json!({}));
                        if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let block = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                            let idx = self.tools.len();
                            let id = cb.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
                            let name = cb.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                            self.tools.push((id.clone(), name.clone(), String::new()));
                            self.block_tool.insert(block, idx);
                            out.push(frame(Format::Openai, "", &self.tool_chunk(idx, Some(&id), Some(&name), "")));
                        }
                    }
                    "content_block_delta" => {
                        self.ensure_started(&mut out);
                        let d = v.get("delta").cloned().unwrap_or(json!({}));
                        if let Some(t) = d.get("text").and_then(|t| t.as_str()) {
                            out.push(frame(Format::Openai, "", &self.chunk(json!({ "content": t }), None)));
                        } else if let Some(pj) = d.get("partial_json").and_then(|t| t.as_str()) {
                            let block = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                            if let Some(&idx) = self.block_tool.get(&block) {
                                self.tools[idx].2.push_str(pj);
                                out.push(frame(Format::Openai, "", &self.tool_chunk(idx, None, None, pj)));
                            }
                        }
                    }
                    "message_delta" => {
                        self.ensure_started(&mut out);
                        let stop = v.get("delta").and_then(|d| d.get("stop_reason")).and_then(|s| s.as_str()).unwrap_or("end_turn");
                        self.pending_finish = Some(stop.to_string());
                    }
                    "message_stop" => out.extend(self.finish()),
                    _ => {}
                }
            }
        }
        out
    }

    pub fn finish(&mut self) -> Vec<Bytes> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        self.ensure_started(&mut out);
        self.finished = true;
        let stop_src = self.pending_finish.clone().unwrap_or_default();
        let mut stop = map_stop(self.from, self.to, &stop_src);
        if !self.tools.is_empty() {
            stop = if self.to == Format::Anthropic { "tool_use".into() } else { "tool_calls".into() };
        }
        // record tool calls on the observation so the span has them
        for (id, name, args) in &self.tools {
            self.obs.observed.tool_calls.push((id.clone(), name.clone(), serde_json::from_str(args).unwrap_or(json!({"_raw": args}))));
        }
        let o = &self.obs.observed;
        match self.to {
            Format::Anthropic => {
                if self.block_open {
                    out.push(frame(Format::Anthropic, "content_block_stop", &json!({ "type": "content_block_stop", "index": 0 })));
                }
                for i in 0..self.tools.len() {
                    out.push(frame(Format::Anthropic, "content_block_stop", &json!({ "type": "content_block_stop", "index": (i as u64) + 1 })));
                }
                out.push(frame(
                    Format::Anthropic,
                    "message_delta",
                    &json!({ "type": "message_delta", "delta": { "stop_reason": if stop.is_empty() { "end_turn".to_string() } else { stop }, "stop_sequence": null },
                             "usage": { "input_tokens": o.input_tokens, "output_tokens": o.output_tokens } }),
                ));
                out.push(frame(Format::Anthropic, "message_stop", &json!({ "type": "message_stop" })));
            }
            Format::Openai => {
                out.push(frame(Format::Openai, "", &self.chunk(json!({}), Some(if stop.is_empty() { "stop" } else { &stop }))));
                let mut usage_chunk = self.chunk(json!({}), None);
                usage_chunk["choices"] = json!([]);
                usage_chunk["usage"] = json!({ "prompt_tokens": o.input_tokens, "completion_tokens": o.output_tokens, "total_tokens": o.input_tokens + o.output_tokens });
                out.push(frame(Format::Openai, "", &usage_chunk));
                out.push(done_frame());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_shapes_into_same_norm() {
        let a = parse_request(Format::Anthropic, &json!({ "model": "assistant", "max_tokens": 100, "system": [{"type":"text","text":"be brief"}],
            "messages": [{"role":"user","content":[{"type":"text","text":"hi"}]}], "stream": true }));
        let o = parse_request(Format::Openai, &json!({ "model": "assistant", "max_completion_tokens": 100,
            "messages": [{"role":"system","content":"be brief"},{"role":"user","content":"hi"}], "stream": true }));
        assert_eq!(a.system, o.system);
        assert_eq!(a.messages, o.messages);
        assert_eq!(a.max_tokens, Some(100));
        assert!(o.stream);
    }

    #[test]
    fn tools_and_images_roundtrip() {
        // Anthropic request with a tool definition, an image and a prior tool_use/tool_result
        let a = json!({ "model": "m", "max_tokens": 10, "tools": [{ "name": "get_weather", "description": "w", "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } } }],
            "tool_choice": { "type": "any" },
            "messages": [
              { "role": "user", "content": [{ "type": "text", "text": "look" }, { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AAAA" } }] },
              { "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": { "city": "Rio" } }] },
              { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "30C" }] }
            ] });
        let n = parse_request(Format::Anthropic, &a);
        assert_eq!(n.tools.len(), 1);
        assert_eq!(n.tool_choice, ToolChoice::Required);
        let o = build_request(Format::Openai, &n, "gpt", false);
        assert_eq!(o["tools"][0]["function"]["name"], "get_weather");
        assert_eq!(o["tool_choice"], "required");
        let msgs = o["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"][1]["type"], "image_url");
        assert!(msgs[0]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "toolu_1");
        // and back: OpenAI → Anthropic
        let n2 = parse_request(Format::Openai, &o);
        let a2 = build_request(Format::Anthropic, &n2, "claude", false);
        assert_eq!(a2["tools"][0]["input_schema"]["properties"]["city"]["type"], "string");
        assert_eq!(a2["tool_choice"]["type"], "any");
        assert_eq!(a2["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(a2["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(a2["messages"][0]["content"][1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn tool_call_responses_translate() {
        let oa = json!({ "id": "c", "model": "gpt", "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "get_weather", "arguments": "{\"city\":\"Rio\"}" } }] }, "finish_reason": "tool_calls" }], "usage": { "prompt_tokens": 1, "completion_tokens": 2 } });
        let obs = observe_response(Format::Openai, &oa);
        assert_eq!(obs.tool_calls.len(), 1);
        let an = build_response(Format::Anthropic, &obs, Format::Openai);
        assert_eq!(an["stop_reason"], "tool_use");
        assert_eq!(an["content"][0]["type"], "tool_use");
        assert_eq!(an["content"][0]["input"]["city"], "Rio");
        let back = observe_response(Format::Anthropic, &an);
        let oa2 = build_response(Format::Openai, &back, Format::Anthropic);
        assert_eq!(oa2["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(oa2["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn stream_tool_calls_openai_to_anthropic() {
        let mut t = StreamTranslator::new(Format::Openai, Format::Anthropic, "m");
        let mut all = Vec::new();
        all.extend(t.feed(r#"{"id":"c","model":"gpt","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#));
        all.extend(t.feed(r#"{"id":"c","model":"gpt","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":"}}]},"finish_reason":null}]}"#));
        all.extend(t.feed(r#"{"id":"c","model":"gpt","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Rio\"}"}}]},"finish_reason":null}]}"#));
        all.extend(t.feed(r#"{"id":"c","model":"gpt","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#));
        all.extend(t.feed(r#"{"id":"c","model":"gpt","choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#));
        let s = all.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<String>();
        assert!(s.contains(r#""type":"tool_use""#) && s.contains(r#""id":"call_1""#) && s.contains(r#""name":"get_weather""#));
        assert!(s.contains(r#""type":"input_json_delta""#) && s.contains("partial_json"), "{s}");
        assert!(s.contains(r#""stop_reason":"tool_use""#));
        assert_eq!(t.obs.observed.tool_calls[0].2["city"], "Rio");
    }

    #[test]
    fn stream_tool_calls_anthropic_to_openai() {
        let mut t = StreamTranslator::new(Format::Anthropic, Format::Openai, "m");
        let mut all = Vec::new();
        all.extend(t.feed(r#"{"type":"message_start","message":{"id":"m1","model":"claude","usage":{"input_tokens":7}}}"#));
        all.extend(t.feed(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}"#));
        all.extend(t.feed(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{"city":"Rio"}"}}"#));
        all.extend(t.feed(r#"{"type":"content_block_stop","index":0}"#));
        all.extend(t.feed(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}"#));
        all.extend(t.feed(r#"{"type":"message_stop"}"#));
        let s = all.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<String>();
        assert!(s.contains(r#""tool_calls":[{"index":0,"type":"function","function":{"arguments":"","name":"get_weather"},"id":"toolu_1"}]"#) || s.contains(r#""name":"get_weather""#));
        assert!(s.contains(r#""finish_reason":"tool_calls""#));
        assert!(s.trim_end().ends_with("data: [DONE]"));
    }

    #[test]
    fn merges_same_role_turns_and_builds_targets() {
        let n = parse_request(Format::Openai, &json!({ "model": "x", "messages": [
            {"role":"user","content":"a"},{"role":"user","content":"b"},{"role":"assistant","content":"c"}] }));
        assert_eq!(n.messages.len(), 2);
        assert_eq!(n.messages[0].content(), "a\nb");
        let ab = build_request(Format::Anthropic, &n, "claude-opus-5", false);
        assert_eq!(ab["max_tokens"], 4096);
        assert_eq!(ab["messages"][1]["role"], "assistant");
        let ob = build_request(Format::Openai, &NormRequest { stream: true, max_tokens: Some(10), ..n.clone() }, "llama3.2", false);
        assert_eq!(ob["stream_options"]["include_usage"], true);
        assert_eq!(ob["max_tokens"], 10);
        let strict = build_request(Format::Openai, &NormRequest { max_tokens: Some(10), ..n }, "gpt-4o", true);
        assert_eq!(strict["max_completion_tokens"], 10);
    }

    #[test]
    fn observes_non_stream_responses() {
        let a = observe_response(Format::Anthropic, &json!({ "id": "msg_1", "model": "claude-opus-5", "stop_reason": "end_turn",
            "content": [{"type":"text","text":"hello"}], "usage": {"input_tokens": 10, "output_tokens": 3, "cache_read_input_tokens": 4} }));
        assert_eq!(a.text, "hello");
        assert_eq!(a.input_tokens, 10);
        assert_eq!(a.cache_read_tokens, 4);
        let o = observe_response(Format::Openai, &json!({ "id": "c1", "model": "gpt-4o", "choices": [{"message": {"role":"assistant","content":"yo"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 1} }));
        assert_eq!(o.text, "yo");
        assert_eq!(o.output_tokens, 1);
        assert_eq!(map_stop(Format::Openai, Format::Anthropic, "length"), "max_tokens");
    }

    #[test]
    fn sse_parser_handles_split_frames() {
        let mut p = SseParser::default();
        assert!(p.feed(b"event: message_start\ndata: {\"a\":1}\n").is_empty());
        let ev = p.feed(b"\ndata: {\"b\":2}\n\n");
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].0, "message_start");
        assert_eq!(ev[1].1, "{\"b\":2}");
    }

    #[test]
    fn stream_observer_anthropic() {
        let mut so = StreamObserver::new(Format::Anthropic);
        so.feed(r#"{"type":"message_start","message":{"id":"m","model":"claude-opus-5","usage":{"input_tokens":7}}}"#);
        so.feed(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#);
        so.feed(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#);
        so.feed(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#);
        assert_eq!(so.observed.text, "Hello");
        assert_eq!((so.observed.input_tokens, so.observed.output_tokens), (7, 2));
        assert_eq!(so.observed.stop_reason, "end_turn");
    }

    #[test]
    fn translate_openai_stream_to_anthropic() {
        let mut t = StreamTranslator::new(Format::Openai, Format::Anthropic, "assistant");
        let mut all = Vec::new();
        all.extend(t.feed(r#"{"id":"c1","model":"llama3.2","choices":[{"index":0,"delta":{"role":"assistant","content":"Hi"},"finish_reason":null}]}"#));
        all.extend(t.feed(r#"{"id":"c1","model":"llama3.2","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}"#));
        all.extend(t.feed(r#"{"id":"c1","model":"llama3.2","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#));
        all.extend(t.feed(r#"{"id":"c1","model":"llama3.2","choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2}}"#));
        all.extend(t.feed("[DONE]"));
        let s = all.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<String>();
        let events: Vec<&str> = s.lines().filter_map(|l| l.strip_prefix("event: ")).collect();
        assert_eq!(events, ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]);
        assert!(s.contains(r#""stop_reason":"end_turn""#));
        assert!(s.contains(r#""output_tokens":2"#));
        assert!(s.contains(r#""model":"llama3.2""#));
        assert_eq!(t.obs.observed.text, "Hi there");
    }

    #[test]
    fn translate_anthropic_stream_to_openai() {
        let mut t = StreamTranslator::new(Format::Anthropic, Format::Openai, "assistant");
        let mut all = Vec::new();
        all.extend(t.feed(r#"{"type":"message_start","message":{"id":"m1","model":"claude-opus-5","usage":{"input_tokens":7}}}"#));
        all.extend(t.feed(r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#));
        all.extend(t.feed(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Yo"}}"#));
        all.extend(t.feed(r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":1}}"#));
        all.extend(t.feed(r#"{"type":"message_stop"}"#));
        let s = all.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect::<String>();
        assert!(s.contains(r#""content":"Yo""#));
        assert!(s.contains(r#""finish_reason":"length""#));
        assert!(s.contains(r#""prompt_tokens":7"#));
        assert!(s.trim_end().ends_with("data: [DONE]"));
    }
}
