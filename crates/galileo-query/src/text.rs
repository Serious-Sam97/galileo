//! A text form of the query DSL, for a keyboard-first editor.
//!
//! `spans | where route = "/x" and status = error | p95(duration_ms), count() by tenant
//!         | having count() > 100 | order p95 desc | limit 20 | compare previous`
//!
//! Stages are separated by `|`. The first stage is the dataset. `parse` returns a `Query`;
//! `stringify` renders a `Query` back, so the editor can show the text form of a built query.

use crate::{CalcOp, Calculation, Combination, Dataset, Derived, Direction, Filter, FilterOp, Having, HavingTarget, Order, Query, QueryError, Result, TimeRange};

fn calc_op(name: &str) -> Option<CalcOp> {
    Some(match name.to_ascii_lowercase().as_str() {
        "count" => CalcOp::Count,
        "count_distinct" | "distinct" | "uniq" => CalcOp::CountDistinct,
        "sum" => CalcOp::Sum,
        "avg" | "mean" => CalcOp::Avg,
        "min" => CalcOp::Min,
        "max" => CalcOp::Max,
        "p50" | "median" => CalcOp::P50,
        "p75" => CalcOp::P75,
        "p90" => CalcOp::P90,
        "p95" => CalcOp::P95,
        "p99" => CalcOp::P99,
        "p999" => CalcOp::P999,
        "heatmap" => CalcOp::Heatmap,
        "rate_per_sec" | "rate" => CalcOp::RatePerSec,
        _ => return None,
    })
}

fn filter_op(sym: &str) -> Option<FilterOp> {
    Some(match sym {
        "=" | "==" => FilterOp::Eq,
        "!=" => FilterOp::Ne,
        ">" => FilterOp::Gt,
        ">=" => FilterOp::Gte,
        "<" => FilterOp::Lt,
        "<=" => FilterOp::Lte,
        "~" | "contains" => FilterOp::Contains,
        "!~" | "not_contains" => FilterOp::NotContains,
        "^" | "starts_with" => FilterOp::StartsWith,
        "exists" => FilterOp::Exists,
        "not_exists" => FilterOp::NotExists,
        _ => return None,
    })
}

/// Split a stage body into `field op value` triples on `and`/`or`, respecting quotes.
fn parse_where(body: &str, q: &mut Query) -> Result<()> {
    let toks = tokenize(body)?;
    let mut i = 0;
    let mut first = true;
    while i < toks.len() {
        if !first {
            match toks[i].to_ascii_lowercase().as_str() {
                "and" => q.filter_combination = Combination::And,
                "or" => q.filter_combination = Combination::Or,
                _ => return Err(QueryError::Invalid(format!("expected 'and'/'or' before '{}'", toks[i]))),
            }
            i += 1;
        }
        first = false;
        let field = toks.get(i).ok_or_else(|| QueryError::Invalid("where: missing field".into()))?.clone();
        i += 1;
        let op_s = toks.get(i).ok_or_else(|| QueryError::Invalid(format!("where: missing operator after '{field}'")))?;
        let op = filter_op(op_s).ok_or_else(|| QueryError::Invalid(format!("where: unknown operator '{op_s}'")))?;
        i += 1;
        if matches!(op, FilterOp::Exists | FilterOp::NotExists) {
            q.filters.push(Filter { field, op, value: None });
            continue;
        }
        let raw = toks.get(i).ok_or_else(|| QueryError::Invalid(format!("where: missing value for '{field}'")))?.clone();
        i += 1;
        let value = if let Ok(n) = raw.parse::<f64>() { serde_json::json!(n) } else if raw == "true" || raw == "false" { serde_json::json!(raw == "true") } else { serde_json::json!(raw) };
        q.filters.push(Filter { field, op, value: Some(value) });
    }
    Ok(())
}

fn parse_calc(item: &str, q: &mut Query) -> Result<String> {
    let item = item.trim();
    if let Some(open) = item.find('(') {
        let name = &item[..open];
        let inner = item[open + 1..].trim_end_matches(')').trim();
        if let Some(op) = calc_op(name) {
            let field = (!inner.is_empty()).then(|| inner.to_string());
            let c = Calculation { op, field };
            let label = c.label();
            q.calculations.push(c);
            return Ok(label);
        }
    }
    // a bare word: treat count specially, else a derived reference must already exist
    if item.eq_ignore_ascii_case("count") { q.calculations.push(Calculation { op: CalcOp::Count, field: None }); return Ok("COUNT".into()); }
    Err(QueryError::Invalid(format!("'{item}' is not an aggregation like p95(duration_ms) or count()")))
}

fn parse_aggregation(body: &str, q: &mut Query) -> Result<()> {
    let (calcs, by) = match body.to_ascii_lowercase().find(" by ") {
        Some(p) => (&body[..p], Some(body[p + 4..].trim())),
        None => (body, None),
    };
    for item in split_commas(calcs) {
        let item = item.trim();
        if item.is_empty() { continue; }
        parse_calc(item, q)?;
    }
    if let Some(by) = by {
        for b in split_commas(by) { let b = b.trim(); if !b.is_empty() { q.breakdowns.push(b.to_string()); } }
    }
    Ok(())
}

fn parse_having(body: &str, q: &mut Query) -> Result<()> {
    let toks = tokenize(body)?;
    if toks.len() < 3 { return Err(QueryError::Invalid("having: expected `<calc> op value`".into())); }
    let op = filter_op(&toks[toks.len() - 2]).ok_or_else(|| QueryError::Invalid("having: unknown operator".into()))?;
    let value: f64 = toks[toks.len() - 1].parse().map_err(|_| QueryError::Invalid("having: value must be a number".into()))?;
    let target = toks[..toks.len() - 2].join("");
    q.having.push(Having { target: HavingTarget(target), op, value });
    Ok(())
}

pub fn parse(input: &str) -> Result<Query> {
    let mut q = Query { calculations: vec![], filters: vec![], breakdowns: vec![], orders: vec![], ..Default::default() };
    let mut stages = input.split('|').map(str::trim).filter(|s| !s.is_empty());
    let head = stages.next().ok_or_else(|| QueryError::Invalid("empty query".into()))?;
    q.dataset = match head.to_ascii_lowercase().as_str() {
        "spans" | "span" | "traces" => Dataset::Spans,
        "logs" | "log" => Dataset::Logs,
        "metrics" | "metric" => Dataset::Metrics,
        other => return Err(QueryError::Invalid(format!("first stage must be a dataset (spans/logs/metrics), got '{other}'"))),
    };
    for stage in stages {
        let (kw, body) = stage.split_once(char::is_whitespace).unwrap_or((stage, ""));
        let body = body.trim();
        match kw.to_ascii_lowercase().as_str() {
            "where" | "filter" => parse_where(body, &mut q)?,
            "having" => parse_having(body, &mut q)?,
            "order" | "sort" => {
                let mut it = body.split_whitespace();
                let field = it.next().ok_or_else(|| QueryError::Invalid("order: missing field".into()))?.to_string();
                let direction = match it.next().map(|s| s.to_ascii_lowercase()).as_deref() { Some("asc") => Direction::Asc, _ => Direction::Desc };
                q.orders.push(Order { field, direction });
            }
            "limit" => q.limit = Some(body.trim().parse().map_err(|_| QueryError::Invalid("limit must be a number".into()))?),
            "search" => q.search = Some(unquote(body)),
            "compare" => q.compare_to = Some(if body.eq_ignore_ascii_case("previous") || body.is_empty() { "previous".into() } else { body.to_string() }),
            "derive" | "let" => {
                let (name, expr) = body.split_once('=').ok_or_else(|| QueryError::Invalid("derive: expected `name = expr`".into()))?;
                q.derived.push(Derived { name: name.trim().to_string(), expr: expr.trim().to_string() });
            }
            // no keyword → an aggregation stage (calcs [by ...])
            _ => parse_aggregation(stage, &mut q)?,
        }
    }
    // `order p95 desc` may name an aggregation by its op alone; resolve it to the calculation label
    for o in q.orders.iter_mut() {
        let f = o.field.clone();
        if q.breakdowns.contains(&f) || q.calculations.iter().any(|c| c.label() == f) || q.derived.iter().any(|d| d.name == f) { continue; }
        let lf = f.to_ascii_lowercase();
        let hit = q.calculations.iter().find(|c| c.label().to_ascii_lowercase() == lf)
            .or_else(|| calc_op(lf.trim_end_matches("()")).and_then(|op| q.calculations.iter().find(|c| c.op == op)));
        if let Some(c) = hit { o.field = c.label(); }
    }
    q.validate()?;
    Ok(q)
}

/// Render a `Query` as the pipe form (best-effort; relative ranges are not encoded here).
pub fn stringify(q: &Query) -> String {
    let mut parts = vec![match q.dataset { Dataset::Spans => "spans", Dataset::Logs => "logs", Dataset::Metrics => "metrics" }.to_string()];
    if !q.filters.is_empty() {
        let joiner = if q.filter_combination == Combination::Or { " or " } else { " and " };
        let fs: Vec<String> = q.filters.iter().map(|f| {
            let op = op_str(f.op);
            match &f.value {
                None => format!("{} {}", f.field, op),
                Some(v) => format!("{} {} {}", f.field, op, val_str(v)),
            }
        }).collect();
        parts.push(format!("where {}", fs.join(joiner)));
    }
    for d in &q.derived { parts.push(format!("derive {} = {}", d.name, d.expr)); }
    if !q.calculations.is_empty() {
        let calcs: Vec<String> = q.calculations.iter().map(|c| c.label().to_lowercase()).collect();
        let mut agg = calcs.join(", ");
        if !q.breakdowns.is_empty() { agg.push_str(&format!(" by {}", q.breakdowns.join(", "))); }
        parts.push(agg);
    }
    for h in &q.having { parts.push(format!("having {} {} {}", h.target.0, op_str(h.op), h.value)); }
    for o in &q.orders { parts.push(format!("order {} {}", o.field, if o.direction == Direction::Asc { "asc" } else { "desc" })); }
    if let Some(l) = q.limit { parts.push(format!("limit {l}")); }
    if let Some(s) = &q.search { parts.push(format!("search \"{s}\"")); }
    if let Some(c) = &q.compare_to { parts.push(format!("compare {c}")); }
    let _ = TimeRange::default();
    parts.join(" | ")
}

fn op_str(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "=", FilterOp::Ne => "!=", FilterOp::Gt => ">", FilterOp::Gte => ">=",
        FilterOp::Lt => "<", FilterOp::Lte => "<=", FilterOp::Contains => "~", FilterOp::NotContains => "!~",
        FilterOp::StartsWith => "^", FilterOp::Exists => "exists", FilterOp::NotExists => "not_exists",
        FilterOp::In => "in", FilterOp::NotIn => "not_in",
    }
}
fn val_str(v: &serde_json::Value) -> String {
    match v { serde_json::Value::String(s) if s.contains(' ') || s.is_empty() => format!("\"{s}\""), serde_json::Value::String(s) => s.clone(), other => other.to_string() }
}
fn unquote(s: &str) -> String { s.trim().trim_matches('"').trim_matches('\'').to_string() }

/// Split on top-level commas (no nesting in our grammar, but keep quotes intact).
fn split_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = None;
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '"' | '\'' if q == Some(c) => { q = None; cur.push(c); }
            '"' | '\'' if q.is_none() => { q = Some(c); cur.push(c); }
            '(' if q.is_none() => { depth += 1; cur.push(c); }
            ')' if q.is_none() => { depth -= 1; cur.push(c); }
            ',' if q.is_none() && depth == 0 => { out.push(std::mem::take(&mut cur)); }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() { out.push(cur); }
    out
}

/// Tokenize a where/having body into words and operators, honoring quotes and multi-char operators.
fn tokenize(s: &str) -> Result<Vec<String>> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() { i += 1; continue; }
        if c == '"' || c == '\'' {
            let q = c; i += 1; let start = i;
            while i < b.len() && b[i] != q { i += 1; }
            if i >= b.len() { return Err(QueryError::Invalid("unterminated string".into())); }
            out.push(b[start..i].iter().collect()); i += 1;
        } else if matches!(c, '=' | '!' | '<' | '>' | '~' | '^') {
            let mut op = String::from(c); i += 1;
            if i < b.len() && (b[i] == '=' || (c == '!' && b[i] == '~')) { op.push(b[i]); i += 1; }
            out.push(op);
        } else {
            let start = i;
            while i < b.len() && !b[i].is_whitespace() && !matches!(b[i], '=' | '!' | '<' | '>' | '~' | '^') { i += 1; }
            out.push(b[start..i].iter().collect());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let q = parse(r#"spans | where http.route = "/x" and status_code = error | p95(duration_ms), count() by tenant_id | having count() > 100 | order p95 desc | limit 20"#).unwrap();
        assert_eq!(q.dataset, Dataset::Spans);
        assert_eq!(q.filters.len(), 2);
        assert_eq!(q.calculations.len(), 2);
        assert_eq!(q.breakdowns, vec!["tenant_id".to_string()]);
        assert_eq!(q.having.len(), 1);
        assert_eq!(q.having[0].value, 100.0);
        assert_eq!(q.limit, Some(20));
        let s = stringify(&q);
        let q2 = parse(&s).unwrap();
        assert_eq!(q2.calculations, q.calculations);
        assert_eq!(q2.breakdowns, q.breakdowns);
        assert_eq!(q2.filters.len(), q.filters.len());
    }
    #[test]
    fn order_by_bare_op_resolves_to_label() {
        let q = parse("spans | p95(duration_ms), count() by http_route | order p95 desc | order count asc").unwrap();
        assert_eq!(q.orders[0].field, "P95(duration_ms)");
        assert_eq!(q.orders[1].field, "COUNT");
    }
    #[test]
    fn compare_and_derive() {
        let q = parse("spans | derive cpt = SUM(gen_ai.usage.cost_usd) / SUM(gen_ai.usage.output_tokens) | sum(gen_ai.usage.cost_usd) by gen_ai_model | compare previous").unwrap();
        assert_eq!(q.derived.len(), 1);
        assert_eq!(q.compare_to.as_deref(), Some("previous"));
    }
    #[test]
    fn errors_have_message() {
        assert!(parse("banana | count()").is_err());
        assert!(parse("spans | frobnicate(x)").is_err());
    }
}
