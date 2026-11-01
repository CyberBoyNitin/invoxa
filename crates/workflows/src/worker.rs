//! Worker loops with graceful shutdown, lease heartbeats, and periodic
//! recovery of expired leases.

use crate::export::ExportWorker;
use crate::pipeline::DocumentPipeline;
use crate::webhook::WebhookWorker;
use invoxa_database::{jobs, operations, Db};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub lease_seconds: i32,
    pub poll_interval: Duration,
    pub recovery_interval: Duration,
    pub document_concurrency: usize,
    pub export_concurrency: usize,
    pub webhook_concurrency: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            lease_seconds: 300,
            poll_interval: Duration::from_millis(1_500),
            recovery_interval: Duration::from_secs(60),
            document_concurrency: 2,
            export_concurrency: 1,
            webhook_concurrency: 2,
        }
    }
}

impl WorkerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(10..=3_600).contains(&self.lease_seconds) {
            return Err("worker lease must be between 10 and 3600 seconds".into());
        }
        if self.document_concurrency > 32
            || self.export_concurrency > 8
            || self.webhook_concurrency > 32
        {
            return Err("worker concurrency is above the supported limit".into());
        }
        if self.poll_interval < Duration::from_millis(100)
            || self.poll_interval > Duration::from_secs(30)
        {
            return Err("worker poll interval must be between 100ms and 30s".into());
        }
        Ok(())
    }
}

pub struct WorkerSet {
    shutdown: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
}

impl WorkerSet {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        for handle in self.handles {
            let _ = handle.await;
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handles.iter().all(|handle| handle.is_finished())
    }
}

pub fn spawn_workers(
    pipeline: Arc<DocumentPipeline>,
    export: Arc<ExportWorker>,
    webhook: Arc<WebhookWorker>,
    db: Db,
    config: WorkerConfig,
    base_worker_id: &str,
) -> Result<WorkerSet, String> {
    config.validate()?;
    let (shutdown, receiver) = watch::channel(false);
    let mut handles = Vec::new();

    for index in 0..config.document_concurrency {
        let pipeline = pipeline.clone();
        let mut receiver = receiver.clone();
        let config = config.clone();
        let worker_id = format!("{base_worker_id}-doc-{index}");
        handles.push(tokio::spawn(async move {
            document_loop(pipeline, config, worker_id, &mut receiver).await;
        }));
    }
    for index in 0..config.export_concurrency {
        let export = export.clone();
        let mut receiver = receiver.clone();
        let config = config.clone();
        let worker_id = format!("{base_worker_id}-export-{index}");
        handles.push(tokio::spawn(async move {
            export_loop(export, config, worker_id, &mut receiver).await;
        }));
    }
    for index in 0..config.webhook_concurrency {
        let webhook = webhook.clone();
        let mut receiver = receiver.clone();
        let config = config.clone();
        let worker_id = format!("{base_worker_id}-webhook-{index}");
        handles.push(tokio::spawn(async move {
            webhook_loop(webhook, config, worker_id, &mut receiver).await;
        }));
    }
    {
        let mut receiver = receiver.clone();
        let config = config.clone();
        handles.push(tokio::spawn(async move {
            recovery_loop(db, config, &mut receiver).await;
        }));
    }

    Ok(WorkerSet { shutdown, handles })
}

async fn document_loop(
    pipeline: Arc<DocumentPipeline>,
    config: WorkerConfig,
    worker_id: String,
    shutdown: &mut watch::Receiver<bool>,
) {
    info!(worker_id = %worker_id, "document worker started");
    loop {
        if *shutdown.borrow() {
            break;
        }
        match jobs::claim_job(pipeline.db(), &worker_id, config.lease_seconds).await {
            Ok(Some(job)) => {
                let job_id = job.job_id;
                let heartbeat =
                    spawn_lease_heartbeat(pipeline.db().clone(), job_id, config.lease_seconds);
                let outcome = pipeline.process_claimed(job).await;
                heartbeat.abort();
                if outcome.status == "retry" || outcome.status == "dead" {
                    warn!(
                        worker_id = %worker_id,
                        job_id = %outcome.job_id,
                        status = outcome.status,
                        message = ?outcome.message,
                        "document job did not succeed"
                    );
                }
            }
            Ok(None) => {
                if sleep_or_shutdown(shutdown, config.poll_interval).await {
                    break;
                }
            }
            Err(error) => {
                error!(worker_id = %worker_id, error = %error, "could not claim document job");
                if sleep_or_shutdown(shutdown, Duration::from_secs(5)).await {
                    break;
                }
            }
        }
    }
    info!(worker_id = %worker_id, "document worker stopped");
}

async fn export_loop(
    export: Arc<ExportWorker>,
    config: WorkerConfig,
    worker_id: String,
    shutdown: &mut watch::Receiver<bool>,
) {
    info!(worker_id = %worker_id, "export worker started");
    loop {
        if *shutdown.borrow() {
            break;
        }
        match export.claim(config.lease_seconds).await {
            Ok(Some(job)) => {
                let outcome = export.process_claimed(job).await;
                if outcome.status != "succeeded" {
                    warn!(
                        worker_id = %worker_id,
                        export_id = %outcome.export_id,
                        message = ?outcome.message,
                        "export job did not succeed"
                    );
                }
            }
            Ok(None) => {
                if sleep_or_shutdown(shutdown, config.poll_interval).await {
                    break;
                }
            }
            Err(error) => {
                error!(worker_id = %worker_id, error = %error, "could not claim export job");
                if sleep_or_shutdown(shutdown, Duration::from_secs(5)).await {
                    break;
                }
            }
        }
    }
    info!(worker_id = %worker_id, "export worker stopped");
}

async fn webhook_loop(
    webhook: Arc<WebhookWorker>,
    config: WorkerConfig,
    worker_id: String,
    shutdown: &mut watch::Receiver<bool>,
) {
    info!(worker_id = %worker_id, "webhook worker started");
    loop {
        if *shutdown.borrow() {
            break;
        }
        match webhook.claim(config.lease_seconds).await {
            Ok(Some(delivery)) => {
                let outcome = webhook.process_claimed(delivery).await;
                if outcome.status != "succeeded" {
                    warn!(
                        worker_id = %worker_id,
                        delivery_id = %outcome.delivery_id,
                        status = outcome.status,
                        retry_in = ?outcome.next_attempt_in_seconds,
                        error = ?outcome.error,
                        "webhook delivery did not succeed"
                    );
                }
            }
            Ok(None) => {
                if sleep_or_shutdown(shutdown, config.poll_interval).await {
                    break;
                }
            }
            Err(error) => {
                error!(worker_id = %worker_id, error = %error, "could not claim webhook delivery");
                if sleep_or_shutdown(shutdown, Duration::from_secs(5)).await {
                    break;
                }
            }
        }
    }
    info!(worker_id = %worker_id, "webhook worker stopped");
}

async fn recovery_loop(db: Db, config: WorkerConfig, shutdown: &mut watch::Receiver<bool>) {
    loop {
        match jobs::recover_expired_jobs(&db).await {
            Ok((retried, dead)) if retried + dead > 0 => {
                warn!(retried, dead, "recovered expired document jobs");
            }
            Ok(_) => {}
            Err(error) => {
                error!(error = %error, "document job recovery failed");
            }
        }
        match operations::recover_webhook_deliveries(&db).await {
            Ok(count) if count > 0 => {
                warn!(count, "recovered expired webhook deliveries");
            }
            Ok(_) => {}
            Err(error) => {
                error!(error = %error, "webhook recovery failed");
            }
        }
        match operations::recover_expired_exports(&db).await {
            Ok(count) if count > 0 => {
                warn!(count, "recovered expired export jobs");
            }
            Ok(_) => {}
            Err(error) => {
                error!(error = %error, "export recovery failed");
            }
        }
        if sleep_or_shutdown(shutdown, config.recovery_interval).await {
            break;
        }
    }
}

fn spawn_lease_heartbeat(db: Db, job_id: uuid::Uuid, lease_seconds: i32) -> JoinHandle<()> {
    tokio::spawn(async move {
        let interval_seconds = (lease_seconds / 3).max(1) as u64;
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
        ticker.tick().await; // consume the immediate first tick
        loop {
            ticker.tick().await;
            match jobs::renew_lease(&db, job_id, lease_seconds).await {
                Ok(true) => {}
                Ok(false) => {
                    warn!(job_id = %job_id, "lease heartbeat stopped: job is no longer running");
                    break;
                }
                Err(error) => {
                    warn!(job_id = %job_id, error = %error, "lease heartbeat failed");
                    break;
                }
            }
        }
    })
}

async fn sleep_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
        _ = tokio::time::sleep(duration) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_config_rejects_unsafe_values() {
        let config = WorkerConfig::default();
        assert!(config.validate().is_ok());
        let mut bad = config.clone();
        bad.lease_seconds = 1;
        assert!(bad.validate().is_err());
        let mut bad = WorkerConfig::default();
        bad.document_concurrency = 99;
        assert!(bad.validate().is_err());
    }
}
