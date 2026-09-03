//! Runs against the docker-compose ClickHouse. `cargo test -p galileo-storage -- --ignored`.

use chrono::Utc;
use galileo_core::config::RetentionConfig;
use galileo_core::*;
use galileo_storage::{ClickHouseStorage, SqlQuery, Storage};

fn cfg() -> galileo_core::config::ClickHouseConfig {
    let c = Config::load(None).unwrap_or_else(|_| Config::default());
    c.clickhouse
}

fn span(project: ProjectId, trace: TraceId, name: &str, parent: Option<SpanId>) -> Span {
    let start = Utc::now();
    let mut attrs = Attributes::new();
    attrs.insert("http.route".into(), "/pets/{id}".into());
    attrs.insert("http.response.status_code".into(), 200i64.into());
    attrs.insert("user.id".into(), "user-42".into());
    attrs.insert("custom.score".into(), 0.75f64.into());
    let mut res = Attributes::new();
    res.insert("service.name".into(), "petshop".into());
    Span {
        project_id: project,
        trace_id: trace,
        span_id: SpanId::random(),
        parent_span_id: parent,
        name: name.into(),
        kind: SpanKind::Server,
        start_time: start,
        end_time: start + chrono::Duration::milliseconds(42),
        status: SpanStatus::default(),
        service_name: "petshop".into(),
        scope_name: "test".into(),
        scope_version: "1".into(),
        resource: res,
        attributes: attrs,
        events: vec![SpanEvent { name: "cache.miss".into(), timestamp: start, attributes: Attributes::new() }],
        links: vec![],
    }
}

#[tokio::test]
#[ignore]
async fn migrate_write_query_roundtrip() {
    let st = ClickHouseStorage::new(&cfg());
    st.ping().await.expect("clickhouse reachable");
    st.migrate().await.expect("migrate");
    st.apply_retention(&RetentionConfig {
        spans: std::time::Duration::from_secs(30 * 86400),
        logs: std::time::Duration::from_secs(14 * 86400),
        metrics: std::time::Duration::from_secs(90 * 86400),
        storage_policy: None,
        hot_days: 7,
    })
    .await
    .expect("retention");

    let project = ProjectId::new();
    let trace = TraceId::random();
    let root = span(project, trace, "GET /pets/{id}", None);
    let child = span(project, trace, "SELECT pets", Some(root.span_id));
    st.write_spans(&[root.clone(), child.clone()]).await.expect("write spans");

    let got = st.fetch_trace(project, trace).await.expect("fetch trace");
    assert_eq!(got.len(), 2);
    let got_root = got.iter().find(|s| s.span_id == root.span_id).unwrap();
    assert_eq!(got_root.attributes["http.response.status_code"], AttributeValue::Int(200));
    assert_eq!(got_root.attributes["custom.score"], AttributeValue::Float(0.75));
    assert_eq!(got_root.events[0].name, "cache.miss");
    assert_eq!(got.iter().find(|s| s.span_id == child.span_id).unwrap().parent_span_id, Some(root.span_id));

    let q = SqlQuery::new(
        "SELECT count() AS n, quantile(0.99)(duration_ns) AS p99, attrs['user.id'] AS u \
         FROM spans WHERE project_id = ? AND trace_id = ? GROUP BY u",
    )
    .bind(project)
    .bind(trace.to_hex());
    let res = st.query(&q).await.expect("query");
    assert_eq!(res.columns[0].name, "n");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], serde_json::json!(2));
    assert_eq!(res.rows[0][2], serde_json::json!("user-42"));

    // logs + metrics write paths
    let log = LogRecord {
        project_id: project,
        timestamp: Utc::now(),
        observed_timestamp: Utc::now(),
        severity: Severity::Warn,
        severity_text: "WARN".into(),
        body: "cache miss for pet 42".into(),
        body_value: None,
        trace_id: Some(trace),
        span_id: Some(root.span_id),
        service_name: "petshop".into(),
        scope_name: "test".into(),
        resource: Attributes::new(),
        attributes: Attributes::new(),
    };
    st.write_logs(&[log]).await.expect("write logs");
    let point = MetricPoint {
        project_id: project,
        name: "http.server.request.duration".into(),
        description: String::new(),
        unit: "s".into(),
        kind: MetricKind::Histogram,
        temporality: AggregationTemporality::Cumulative,
        is_monotonic: false,
        timestamp: Utc::now(),
        start_timestamp: None,
        service_name: "petshop".into(),
        scope_name: "test".into(),
        resource: Attributes::new(),
        attributes: Attributes::new(),
        value: 0.0,
        count: 3,
        sum: 0.3,
        min: Some(0.05),
        max: Some(0.2),
        bucket_counts: vec![1, 2, 0],
        explicit_bounds: vec![0.1, 0.5],
        exp_scale: 0, exp_zero_count: 0, exp_pos_offset: 0, exp_pos_counts: vec![], exp_neg_offset: 0, exp_neg_counts: vec![],
    };
    st.write_metrics(&[point]).await.expect("write metrics");

    let n = st
        .query(&SqlQuery::new("SELECT count() FROM logs WHERE project_id = ? AND hasToken(body, 'miss')").bind(project))
        .await
        .unwrap();
    assert_eq!(n.rows[0][0], serde_json::json!(1));

    // non-select is refused
    assert!(st.query(&SqlQuery::new("DROP TABLE spans")).await.is_err());
}
