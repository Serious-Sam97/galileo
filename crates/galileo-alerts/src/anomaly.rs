//! Cost anomaly detection for gateway routes: this hour's spend vs the same hour over the previous
//! seven days. Writes a `budget_events` row (kind = anomaly) once per route per hour; the budget
//! notifier delivers it.

use chrono::{Timelike, Utc};
use galileo_storage::SqlQuery;
use uuid::Uuid;

use crate::Evaluator;

pub async fn run_due(ev: &Evaluator) -> anyhow::Result<()> {
    let now = Utc::now();
    // current hour spend per project/route plus the 7 previous same-hour values
    let res = ev.storage.query(&SqlQuery { sql: "
        WITH toStartOfHour(now()) AS h0
        SELECT project_id, attrs['gen_ai.galileo.route'] AS route,
               sumIf(gen_ai_cost_usd, timestamp >= h0) AS cur,
               arraySort(groupArrayIf(s, d > 0)) AS hist
        FROM (
            SELECT project_id, attrs, gen_ai_cost_usd, timestamp,
                   toInt32((toStartOfHour(now()) - toStartOfHour(timestamp)) / 86400) AS d,
                   sum(gen_ai_cost_usd) OVER (PARTITION BY project_id, attrs['gen_ai.galileo.route'], toStartOfHour(timestamp)) AS s
            FROM spans
            WHERE gen_ai_system != '' AND attrs['gen_ai.galileo.route'] != ''
              AND timestamp >= toStartOfHour(now()) - INTERVAL 7 DAY
              AND (timestamp >= toStartOfHour(now()) OR (toHour(timestamp) = toHour(now()) AND timestamp < toStartOfHour(now())))
        )
        GROUP BY project_id, route
        HAVING cur > 0".into(), params: vec![] }).await?;
    for r in &res.rows {
        let project: Uuid = match r.first().and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()) { Some(p) => p, None => continue };
        let route = r.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cur = r.get(2).and_then(num).unwrap_or(0.0);
        let hist: Vec<f64> = r.get(3).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(num).collect()).unwrap_or_default();
        if hist.len() < 3 { continue; } // need a baseline
        let median = hist[hist.len() / 2];
        let threshold = (median * 3.0).max(median + 0.5);
        if cur <= threshold { continue; }
        let period = format!("hour:{}", now.format("%Y-%m-%dT%H"));
        let _ = sqlx::query("INSERT INTO budget_events (id, project_id, route_alias, kind, period, spent_usd, cap_usd) VALUES ($1, $2, $3, 'anomaly', $4, $5, $6) ON CONFLICT DO NOTHING")
            .bind(Uuid::now_v7()).bind(project).bind(&route).bind(&period).bind(cur).bind(threshold).execute(&ev.pg).await;
    }
    let _ = now.hour();
    Ok(())
}

fn num(v: &serde_json::Value) -> Option<f64> { v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())) }
