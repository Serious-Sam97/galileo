//! Batching writer: receivers push rows into a bounded channel; one background task groups
//! them by table and flushes on size or time. Backpressure is a full channel, which the
//! receivers turn into 429 / RESOURCE_EXHAUSTED so SDKs retry with backoff instead of losing data.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use galileo_core::config::IngestConfig;
use galileo_core::{LogRecord, MetricPoint, Span};
use galileo_storage::DynStorage;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, warn};

pub enum Batch {
    Spans(Vec<Span>),
    /// Spans subject to tail sampling under the given policy (rate < 1).
    SampledSpans(galileo_core::ProjectId, galileo_core::Sampling, Vec<Span>),
    Logs(Vec<LogRecord>),
    Metrics(Vec<MetricPoint>),
}

impl Batch {
    fn len(&self) -> usize {
        match self {
            Batch::Spans(v) => v.len(),
            Batch::SampledSpans(_, _, v) => v.len(),
            Batch::Logs(v) => v.len(),
            Batch::Metrics(v) => v.len(),
        }
    }
}

#[derive(Default, Debug)]
pub struct IngestStats {
    pub spans_received: AtomicU64,
    pub logs_received: AtomicU64,
    pub metrics_received: AtomicU64,
    pub rows_written: AtomicU64,
    pub rows_dropped: AtomicU64,
    pub rejected_backpressure: AtomicU64,
    pub write_errors: AtomicU64,
    pub auth_failures: AtomicU64,
    pub sampled_kept: AtomicU64,
    pub sampled_dropped: AtomicU64,
    pub sampled_buffered: AtomicU64,
    pub queued_rows: AtomicU64,
    /// Unix seconds of the last successful write.
    pub last_write_at: AtomicU64,
    /// Rows accepted per (project, day, signal) for quotas; resets on restart (the evaluator
    /// re-counts from ClickHouse for warnings).
    pub quota: dashmap::DashMap<(uuid::Uuid, String, &'static str), u64>,
    pub quota_rejected: AtomicU64,
}

impl IngestStats {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "spans_received": self.spans_received.load(Ordering::Relaxed),
            "logs_received": self.logs_received.load(Ordering::Relaxed),
            "metrics_received": self.metrics_received.load(Ordering::Relaxed),
            "rows_written": self.rows_written.load(Ordering::Relaxed),
            "rows_dropped": self.rows_dropped.load(Ordering::Relaxed),
            "rejected_backpressure": self.rejected_backpressure.load(Ordering::Relaxed),
            "write_errors": self.write_errors.load(Ordering::Relaxed),
            "auth_failures": self.auth_failures.load(Ordering::Relaxed),
            "sampled_kept": self.sampled_kept.load(Ordering::Relaxed),
            "sampled_dropped": self.sampled_dropped.load(Ordering::Relaxed),
            "sampled_buffered": self.sampled_buffered.load(Ordering::Relaxed),
            "queued_rows": self.queued_rows.load(Ordering::Relaxed),
            "last_write_at": self.last_write_at.load(Ordering::Relaxed),
            "quota_rejected": self.quota_rejected.load(Ordering::Relaxed),
        })
    }
}

impl IngestStats {
    /// Count `n` rows for today; returns the new total.
    pub fn quota_add(&self, project: uuid::Uuid, signal: &'static str, n: u64) -> u64 {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let mut e = self.quota.entry((project, day, signal)).or_insert(0);
        *e += n;
        *e
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("ingest queue full")]
    Full,
    #[error("ingest writer stopped")]
    Closed,
}

/// Cheap handle held by the receivers.
#[derive(Clone)]
pub struct WriterHandle {
    tx: mpsc::Sender<Batch>,
    pub stats: Arc<IngestStats>,
}

impl WriterHandle {
    pub fn push(&self, batch: Batch) -> Result<(), WriteError> {
        let n = batch.len() as u64;
        match &batch {
            Batch::Spans(_) | Batch::SampledSpans(..) => self.stats.spans_received.fetch_add(n, Ordering::Relaxed),
            Batch::Logs(_) => self.stats.logs_received.fetch_add(n, Ordering::Relaxed),
            Batch::Metrics(_) => self.stats.metrics_received.fetch_add(n, Ordering::Relaxed),
        };
        match self.tx.try_send(batch) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.stats.rejected_backpressure.fetch_add(n, Ordering::Relaxed);
                Err(WriteError::Full)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(WriteError::Closed),
        }
    }
}

pub struct BatchWriter {
    handle: WriterHandle,
    task: tokio::task::JoinHandle<()>,
}

impl BatchWriter {
    pub fn start(storage: DynStorage, cfg: &IngestConfig) -> Self {
        // Each channel slot is one export request (typically tens to hundreds of rows), so the
        // slot count is derived from the row cap rather than equal to it.
        let slots = (cfg.max_queued_rows / 64).clamp(64, 65_536);
        let (tx, rx) = mpsc::channel::<Batch>(slots);
        let stats = Arc::new(IngestStats::default());
        let handle = WriterHandle { tx, stats: stats.clone() };
        let task = tokio::spawn(run(storage, rx, stats, cfg.batch_max_rows, cfg.batch_max_wait));
        Self { handle, task }
    }

    pub fn handle(&self) -> WriterHandle {
        self.handle.clone()
    }

    /// Stop accepting, flush what is queued, and wait for the task.
    pub async fn shutdown(self) {
        drop(self.handle);
        if let Err(e) = self.task.await {
            error!(error = %e, "ingest writer task panicked");
        }
    }
}

struct Pending {
    spans: Vec<Span>,
    logs: Vec<LogRecord>,
    metrics: Vec<MetricPoint>,
}

impl Pending {
    fn total(&self) -> usize {
        self.spans.len() + self.logs.len() + self.metrics.len()
    }
}

async fn run(
    storage: DynStorage,
    mut rx: mpsc::Receiver<Batch>,
    stats: Arc<IngestStats>,
    max_rows: usize,
    max_wait: Duration,
) {
    let mut pending = Pending { spans: Vec::new(), logs: Vec::new(), metrics: Vec::new() };
    let mut sampler = crate::sampling::Sampler::default();
    let mut tick = interval(max_wait.max(Duration::from_millis(50)));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_sample_drain = std::time::Instant::now();
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(Batch::Spans(v)) => pending.spans.extend(v),
                Some(Batch::SampledSpans(project, policy, v)) => {
                    sampler.push(project, &policy, v, std::time::Instant::now());
                    stats.sampled_buffered.store(sampler.buffered() as u64, Ordering::Relaxed);
                }
                Some(Batch::Logs(v)) => pending.logs.extend(v),
                Some(Batch::Metrics(v)) => pending.metrics.extend(v),
                None => {
                    pending.spans.extend(sampler.drain(std::time::Instant::now(), true));
                    flush(&storage, &mut pending, &stats).await;
                    info!("ingest writer stopped");
                    return;
                }
            },
            _ = tick.tick() => {
                let now = std::time::Instant::now();
                if sampler.buffered() > 0 && now.duration_since(last_sample_drain) >= Duration::from_secs(1) {
                    last_sample_drain = now;
                    let kept = sampler.drain(now, false);
                    if !kept.is_empty() { pending.spans.extend(kept); }
                    stats.sampled_kept.store(sampler.kept, Ordering::Relaxed);
                    stats.sampled_dropped.store(sampler.dropped, Ordering::Relaxed);
                    stats.sampled_buffered.store(sampler.buffered() as u64, Ordering::Relaxed);
                }
                if pending.total() > 0 {
                    flush(&storage, &mut pending, &stats).await;
                }
                continue;
            }
        }
        stats.queued_rows.store(pending.total() as u64, Ordering::Relaxed);
        if pending.total() >= max_rows {
            flush(&storage, &mut pending, &stats).await;
        }
    }
}

async fn flush(storage: &DynStorage, p: &mut Pending, stats: &IngestStats) {
    async fn one<T>(
        name: &str,
        rows: &mut Vec<T>,
        stats: &IngestStats,
        f: impl std::future::Future<Output = galileo_storage::Result<()>>,
    ) {
        if rows.is_empty() {
            return;
        }
        let n = rows.len() as u64;
        match f.await {
            Ok(()) => {
                stats.rows_written.fetch_add(n, Ordering::Relaxed);
                stats.last_write_at.store(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), Ordering::Relaxed);
            }
            Err(e) => {
                // One retry after a short pause covers ClickHouse restarts / merges; then drop so
                // the queue never wedges. Dropped rows are counted and logged loudly.
                warn!(table = name, error = %e, rows = n, "write failed, retrying once");
                tokio::time::sleep(Duration::from_millis(500)).await;
                stats.write_errors.fetch_add(1, Ordering::Relaxed);
                stats.rows_dropped.fetch_add(n, Ordering::Relaxed);
                error!(table = name, rows = n, "dropping rows after failed write");
            }
        }
        rows.clear();
    }
    // Retry inline: build the future twice is awkward with borrows, so do it explicitly.
    if !p.spans.is_empty() {
        let res = storage.write_spans(&p.spans).await;
        let res = match res {
            Ok(()) => Ok(()),
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                storage.write_spans(&p.spans).await
            }
        };
        one("spans", &mut p.spans, stats, async { res }).await;
    }
    if !p.logs.is_empty() {
        let res = match storage.write_logs(&p.logs).await {
            Ok(()) => Ok(()),
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                storage.write_logs(&p.logs).await
            }
        };
        one("logs", &mut p.logs, stats, async { res }).await;
    }
    if !p.metrics.is_empty() {
        let res = match storage.write_metrics(&p.metrics).await {
            Ok(()) => Ok(()),
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                storage.write_metrics(&p.metrics).await
            }
        };
        one("metrics", &mut p.metrics, stats, async { res }).await;
    }
}
