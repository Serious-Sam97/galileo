//! The Galileo binary: one process hosting OTLP ingest, the HTTP API, the LLM gateway and the
//! alert evaluator.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use galileo_api::{AppState, PgResolver};
use galileo_core::Config;
use galileo_otlp::{ApiKeyResolver, BatchWriter};
use galileo_storage::{ClickHouseStorage, DynStorage, Storage};
use sqlx::postgres::PgPoolOptions;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,galileo=debug")))
        .with(fmt::layer().with_target(true))
        .init();

    let config_path = std::env::args().nth(1).map(PathBuf::from);
    let config = Arc::new(Config::load(config_path.as_deref()).context("loading configuration")?);
    if config.is_default_secret() {
        warn!("server.secret_key is the all-zero default; set a real key before exposing Galileo");
    }
    let secret = config.secret_key_bytes().map_err(anyhow::Error::msg)?;

    // --- storage ---------------------------------------------------------------------------
    let clickhouse = ClickHouseStorage::new(&config.clickhouse);
    clickhouse.ping().await.context("connecting to ClickHouse")?;
    clickhouse.migrate().await.context("migrating ClickHouse schema")?;
    clickhouse.apply_retention(&config.retention).await.context("applying retention")?;
    let storage: DynStorage = Arc::new(clickhouse);
    info!(url = %config.clickhouse.url, "clickhouse ready");

    let pg = PgPoolOptions::new()
        .max_connections(config.postgres.max_connections)
        .connect(&config.postgres.url)
        .await
        .context("connecting to Postgres")?;
    galileo_api::migrate(&pg).await.context("migrating Postgres schema")?;
    info!("postgres ready");

    // --- ingest ----------------------------------------------------------------------------
    galileo_query::rollup::set_raw_retention(config.retention.spans.as_secs() as i64);
    let writer = BatchWriter::start(storage.clone(), &config.ingest);
    let resolver = Arc::new(PgResolver::new(pg.clone()));
    let dyn_resolver: Arc<dyn ApiKeyResolver> = resolver.clone();

    let grpc = galileo_otlp::grpc::OtlpGrpc::new(dyn_resolver.clone(), writer.handle()).router();
    let grpc_addr = config.server.otlp_grpc_addr;
    let grpc_task = tokio::spawn(async move {
        info!(%grpc_addr, "otlp/grpc listening");
        tonic::transport::Server::builder()
            .add_routes(grpc)
            .serve_with_shutdown(grpc_addr, shutdown_signal())
            .await
    });

    let http = galileo_otlp::http::OtlpHttp::new(dyn_resolver.clone(), writer.handle()).router();
    let http_addr = config.server.otlp_http_addr;
    let http_task = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr).await?;
        info!(%http_addr, "otlp/http listening");
        axum::serve(listener, http).with_graceful_shutdown(shutdown_signal()).await
    });

    // --- api + gateway ---------------------------------------------------------------------
    let gateway = galileo_gateway::Gateway::new(pg.clone(), storage.clone(), dyn_resolver.clone(), writer.handle(), secret);
    let alerts = galileo_alerts::Evaluator::new(pg.clone(), storage.clone(), config.smtp.clone(), config.public_url.clone());
    let alerts_task = alerts.clone().start(config.alerts.tick);
    let state = AppState {
        config: config.clone(),
        pg: pg.clone(),
        storage: storage.clone(),
        resolver: resolver.clone(),
        started_at: std::time::Instant::now(),
        secret,
        ingest_stats: writer.handle().stats.clone(),
        gateway,
        alerts,
    };
    // --- self-health gauges into the Default project ------------------------------------
    {
        let st = state.clone();
        let handle = writer.handle();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tick.tick().await;
                let target: Option<(uuid::Uuid,)> = sqlx::query_as("SELECT id FROM projects WHERE name = 'Default' ORDER BY created_at LIMIT 1").fetch_optional(&st.pg).await.unwrap_or(None);
                let Some((pid,)) = target else { continue };
                let points = galileo_api::routes::system::health_points(&st, galileo_core::ProjectId(pid)).await;
                let _ = handle.push(galileo_otlp::writer::Batch::Metrics(points));
            }
        });
    }
    let api = galileo_api::router(state);
    let api_addr = config.server.api_addr;
    let api_task = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(api_addr).await?;
        info!(%api_addr, "api listening");
        axum::serve(listener, api).with_graceful_shutdown(shutdown_signal()).await
    });

    let (g, h, a) = tokio::join!(grpc_task, http_task, api_task);
    g.context("grpc task")?.context("grpc server")?;
    h.context("http task")?.context("http server")?;
    a.context("api task")?.context("api server")?;
    alerts_task.abort();

    info!("flushing ingest queue");
    writer.shutdown().await;
    info!("bye");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("ctrl-c handler") };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate()).expect("sigterm handler").recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    info!("shutdown signal received");
}
