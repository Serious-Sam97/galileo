//! Alerting: triggers (a query + a threshold) and SLOs (good/total over a window with burn-rate
//! alerts). One evaluator task wakes on a tick, evaluates what is due, persists state and
//! notifies on state changes only.

pub mod issues;
pub mod anomaly;
pub mod budgets;
pub mod quotas;
pub mod digest;
pub mod incidents;
pub mod notify;
pub mod retention;
pub mod slos;
pub mod stats;
pub mod triggers;

use std::sync::Arc;
use std::time::Duration;

use galileo_storage::DynStorage;
use sqlx::PgPool;
use tracing::{error, info};

pub struct Evaluator {
    pub pg: PgPool,
    pub storage: DynStorage,
    pub http: reqwest::Client,
    pub mailer: notify::Mailer,
    pub public_url: String,
}

impl Evaluator {
    pub fn new(pg: PgPool, storage: DynStorage, smtp: galileo_core::config::SmtpConfig, public_url: String) -> Arc<Self> {
        Arc::new(Self {
            pg,
            storage,
            http: reqwest::Client::builder().timeout(Duration::from_secs(15)).build().expect("reqwest"),
            mailer: notify::Mailer { cfg: smtp },
            public_url: public_url.trim_end_matches('/').to_string(),
        })
    }

    /// Resolve recipients and send; returns how many targets accepted the message.
    pub async fn notify(&self, project: uuid::Uuid, recipients: &serde_json::Value, n: &notify::Notification) -> usize {
        let targets = notify::resolve(&self.pg, project, &notify::parse_recipients(recipients)).await;
        if targets.is_empty() { return 0; }
        notify::send_all(&self.http, &self.mailer, &targets, n).await
    }

    /// Runs forever; abort the returned handle on shutdown.
    pub fn start(self: Arc<Self>, tick: Duration) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!(?tick, "alert evaluator started");
            let mut interval = tokio::time::interval(tick.max(Duration::from_secs(5)));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Err(e) = triggers::evaluate_due(&self).await {
                    error!(error = %e, "trigger evaluation failed");
                }
                if let Err(e) = slos::evaluate_due(&self).await {
                    error!(error = %e, "slo evaluation failed");
                }
                if let Err(e) = issues::evaluate_due(&self).await {
                    error!(error = %e, "issue evaluation failed");
                }
                if let Err(e) = retention::run_due(&self).await {
                    error!(error = %e, "retention run failed");
                }
                if let Err(e) = digest::run_due(&self).await {
                    error!(error = %e, "digest run failed");
                }
                if let Err(e) = anomaly::run_due(&self).await {
                    error!(error = %e, "cost anomaly check failed");
                }
                if let Err(e) = quotas::run_due(&self).await {
                    error!(error = %e, "quota check failed");
                }
                if let Err(e) = budgets::run_due(&self).await {
                    error!(error = %e, "budget notifications failed");
                }
            }
        })
    }
}
