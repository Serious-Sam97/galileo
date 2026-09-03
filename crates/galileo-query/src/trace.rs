//! Assemble a trace's spans into the shape the waterfall renders: flat list in start order
//! with depth and children, plus a summary.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use galileo_core::{Span, SpanId, TraceId};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SpanNode {
    #[serde(flatten)]
    pub span: Span,
    pub duration_ms: f64,
    pub depth: u32,
    pub children: Vec<SpanId>,
    /// Offset from trace start, ms.
    pub offset_ms: f64,
    /// True when the parent id points at a span we do not have (partial trace).
    pub orphan: bool,
}

/// The same statement issued from the same call site many times in one trace.
#[derive(Debug, Clone, Serialize)]
pub struct RepeatedQuery {
    pub statement: String,
    pub table: String,
    pub function: String,
    pub namespace: String,
    pub count: usize,
    pub total_ms: f64,
    pub span_ids: Vec<SpanId>,
}

pub const N_PLUS_ONE_THRESHOLD: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct TraceView {
    pub trace_id: TraceId,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub duration_ms: f64,
    pub span_count: usize,
    pub error_count: usize,
    pub services: Vec<String>,
    pub roots: Vec<SpanId>,
    pub root_name: String,
    pub llm_calls: usize,
    pub llm_cost_usd: f64,
    pub db_calls: usize,
    pub db_ms: f64,
    pub repeated_queries: Vec<RepeatedQuery>,
    pub spans: Vec<SpanNode>,
}

fn attr_str<'a>(s: &'a Span, k: &str) -> &'a str {
    s.attributes.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

/// Normalise a statement so parameter differences do not split a group: numbers, quoted
/// strings and IN-lists collapse to `?`.
pub fn normalize_sql(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                out.push('?');
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                }
            }
            '0'..='9' => {
                out.push('?');
                while matches!(chars.peek(), Some('0'..='9' | '.')) {
                    chars.next();
                }
            }
            c if c.is_whitespace() => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            c => out.push(c),
        }
    }
    out.trim().chars().take(400).collect()
}

fn repeated_queries(spans: &[Span]) -> (usize, f64, Vec<RepeatedQuery>) {
    let mut groups: HashMap<(String, String), RepeatedQuery> = HashMap::new();
    let mut db_calls = 0;
    let mut db_ms = 0.0;
    for s in spans {
        if s.attributes.get("db.system").is_none() {
            continue;
        }
        db_calls += 1;
        let ms = s.duration_ns() as f64 / 1e6;
        db_ms += ms;
        let stmt = normalize_sql(attr_str(s, "db.query.text").max(attr_str(s, "db.statement")));
        let function = attr_str(s, "code.function.name").to_string();
        let namespace = attr_str(s, "code.namespace").to_string();
        let e = groups.entry((stmt.clone(), format!("{namespace}.{function}"))).or_insert_with(|| RepeatedQuery {
            statement: stmt,
            table: attr_str(s, "db.table").to_string(),
            function,
            namespace,
            count: 0,
            total_ms: 0.0,
            span_ids: vec![],
        });
        e.count += 1;
        e.total_ms += ms;
        e.span_ids.push(s.span_id);
    }
    let mut out: Vec<RepeatedQuery> = groups.into_values().filter(|g| g.count >= N_PLUS_ONE_THRESHOLD).collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.count));
    (db_calls, db_ms, out)
}

pub fn assemble(trace_id: TraceId, mut spans: Vec<Span>) -> TraceView {
    spans.sort_by_key(|s| s.start_time);
    let ids: BTreeSet<SpanId> = spans.iter().map(|s| s.span_id).collect();
    let start = spans.iter().map(|s| s.start_time).min().unwrap_or_else(Utc::now);
    let end = spans.iter().map(|s| s.end_time).max().unwrap_or(start);

    let mut children: HashMap<SpanId, Vec<SpanId>> = HashMap::new();
    let mut roots = Vec::new();
    for s in &spans {
        match s.parent_span_id {
            Some(p) if ids.contains(&p) => children.entry(p).or_default().push(s.span_id),
            _ => roots.push(s.span_id),
        }
    }

    // depth via DFS from roots (spans already start-sorted so children lists are ordered)
    let mut depth: HashMap<SpanId, u32> = HashMap::new();
    let mut stack: Vec<(SpanId, u32)> = roots.iter().map(|r| (*r, 0)).collect();
    while let Some((id, d)) = stack.pop() {
        if depth.insert(id, d).is_some() {
            continue;
        }
        if let Some(ch) = children.get(&id) {
            for c in ch {
                stack.push((*c, d + 1));
            }
        }
    }

    let services: BTreeSet<String> = spans.iter().map(|s| s.service_name.clone()).collect();
    let error_count = spans.iter().filter(|s| s.status.code == galileo_core::StatusCode::Error).count();
    let llm_calls = spans.iter().filter(|s| s.attr("gen_ai.system").is_some() || s.attributes.contains_key("gen_ai.request.model")).count();
    let llm_cost_usd: f64 = spans.iter().map(|s| s.attributes.get("gen_ai.usage.cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0)).sum();
    let (db_calls, db_ms, repeated) = repeated_queries(&spans);
    let root_name = roots
        .first()
        .and_then(|r| spans.iter().find(|s| s.span_id == *r))
        .map(|s| s.name.clone())
        .unwrap_or_default();

    let nodes = spans
        .into_iter()
        .map(|s| {
            let orphan = matches!(s.parent_span_id, Some(p) if !ids.contains(&p));
            SpanNode {
                duration_ms: s.duration_ns() as f64 / 1e6,
                depth: depth.get(&s.span_id).copied().unwrap_or(0),
                children: children.remove(&s.span_id).unwrap_or_default(),
                offset_ms: (s.start_time - start).num_nanoseconds().unwrap_or(0) as f64 / 1e6,
                orphan,
                span: s,
            }
        })
        .collect::<Vec<_>>();

    TraceView {
        trace_id,
        start,
        end,
        duration_ms: (end - start).num_nanoseconds().unwrap_or(0) as f64 / 1e6,
        span_count: nodes.len(),
        error_count,
        services: services.into_iter().collect(),
        roots,
        root_name,
        llm_calls,
        llm_cost_usd,
        db_calls,
        db_ms,
        repeated_queries: repeated,
        spans: nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galileo_core::*;

    fn sp(trace: TraceId, id: u8, parent: Option<u8>, off_ms: i64, dur_ms: i64) -> Span {
        let base = DateTime::parse_from_rfc3339("2026-09-02T12:00:00Z").unwrap().with_timezone(&Utc);
        Span {
            project_id: ProjectId::new(),
            trace_id: trace,
            span_id: SpanId([id; 8]),
            parent_span_id: parent.map(|p| SpanId([p; 8])),
            name: format!("span{id}"),
            kind: SpanKind::Internal,
            start_time: base + chrono::Duration::milliseconds(off_ms),
            end_time: base + chrono::Duration::milliseconds(off_ms + dur_ms),
            status: SpanStatus::default(),
            service_name: "svc".into(),
            scope_name: String::new(),
            scope_version: String::new(),
            resource: Attributes::new(),
            attributes: Attributes::new(),
            events: vec![],
            links: vec![],
        }
    }

    #[test]
    fn normalizes_sql_and_finds_repeats() {
        assert_eq!(normalize_sql("SELECT * FROM pets WHERE id = 42 AND name = 'Rex'"), "SELECT * FROM pets WHERE id = ? AND name = ?");
        let t = TraceId::random();
        let mut spans: Vec<Span> = (0..6)
            .map(|i| {
                let mut s = sp(t, 10 + i, Some(1), i as i64, 2);
                s.attributes.insert("db.system".into(), "postgresql".into());
                s.attributes.insert("db.statement".into(), format!("SELECT * FROM pets WHERE id = {i}").into());
                s.attributes.insert("code.function.name".into(), "list".into());
                s.attributes.insert("code.namespace".into(), "pets.views.PetViewSet".into());
                s
            })
            .collect();
        spans.push(sp(t, 1, None, 0, 100));
        let v = assemble(t, spans);
        assert_eq!(v.db_calls, 6);
        assert_eq!(v.repeated_queries.len(), 1);
        assert_eq!(v.repeated_queries[0].count, 6);
        assert_eq!(v.repeated_queries[0].function, "list");
    }

    #[test]
    fn builds_tree_with_depths_and_orphans() {
        let t = TraceId::random();
        let v = assemble(t, vec![sp(t, 3, Some(2), 20, 5), sp(t, 1, None, 0, 100), sp(t, 2, Some(1), 10, 50), sp(t, 9, Some(7), 30, 1)]);
        assert_eq!(v.span_count, 4);
        assert_eq!(v.roots.len(), 2, "real root + orphan");
        assert_eq!(v.root_name, "span1");
        assert_eq!(v.duration_ms, 100.0);
        let by: HashMap<u8, &SpanNode> = v.spans.iter().map(|n| (n.span.span_id.0[0], n)).collect();
        assert_eq!(by[&1].depth, 0);
        assert_eq!(by[&2].depth, 1);
        assert_eq!(by[&3].depth, 2);
        assert_eq!(by[&3].offset_ms, 20.0);
        assert!(by[&9].orphan);
        assert_eq!(by[&1].children, vec![SpanId([2; 8])]);
    }
}
