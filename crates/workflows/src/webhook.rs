//! Signed webhook delivery worker. Secrets are encrypted at rest and only
//! decrypted in memory for a single delivery. URL validation, DNS pinning,
//! redirect refusal, retry policy, and HMAC signing live in
//! `invoxa-integrations`.

use chrono::Utc;
use invoxa_database::{operations, Db};
use invoxa_integrations::webhook::{retry_delay_seconds, WebhookSender, WEBHOOK_API_VERSION};
use invoxa_security::SecretCipher;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WebhookOutcome {
    pub delivery_id: Uuid,
    pub status: &'static str,
    pub attempts: i32,
    pub next_attempt_in_seconds: Option<u64>,
    pub error: Option<String>,
}

pub struct WebhookWorker {
    db: Db,
    cipher: Arc<SecretCipher>,
    sender: Arc<WebhookSender>,
    worker_id: String,
}

impl WebhookWorker {
    pub fn new(
        db: Db,
        cipher: Arc<SecretCipher>,
        sender: Arc<WebhookSender>,
        worker_id: impl Into<String>,
    ) -> Self {
        Self {
            db,
            cipher,
            sender,
            worker_id: worker_id.into(),
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    pub async fn claim(
        &self,
        lease_seconds: i32,
    ) -> Result<Option<operations::ClaimedDeliveryRow>, sqlx::Error> {
        operations::claim_webhook_delivery(&self.db, &self.worker_id, lease_seconds).await
    }

    pub async fn process_claimed(
        &self,
        delivery: operations::ClaimedDeliveryRow,
    ) -> WebhookOutcome {
        let secret = match std::str::from_utf8(&delivery.secret_ciphertext) {
            Ok(envelope) => match self.cipher.decrypt(envelope) {
                Ok(secret) => secret,
                Err(error) => {
                    error!(delivery_id = %delivery.id, error = %error, "webhook secret decryption failed");
                    return self
                        .dead_letter(
                            &delivery,
                            "WEBHOOK_SECRET_INVALID: the endpoint secret could not be decrypted",
                        )
                        .await;
                }
            },
            Err(_) => {
                return self
                    .dead_letter(
                        &delivery,
                        "WEBHOOK_SECRET_INVALID: the stored endpoint secret is not valid UTF-8",
                    )
                    .await;
            }
        };

        let outcome = self
            .sender
            .send(
                &delivery.endpoint_url,
                secret.as_bytes(),
                delivery.event_id,
                &delivery.event_type,
                &delivery.payload,
                WEBHOOK_API_VERSION,
            )
            .await;

        if let Err(error) = operations::record_delivery_attempt(
            &self.db,
            delivery.organization_id,
            delivery.id,
            delivery.attempts,
            outcome.status_code.map(i32::from),
            outcome.error.as_deref(),
            outcome.response_snippet.as_deref(),
            outcome.duration_ms,
        )
        .await
        {
            error!(delivery_id = %delivery.id, error = %error, "could not record webhook attempt");
        }

        let combined_error = match (&outcome.error_code, &outcome.error) {
            (Some(code), Some(message)) => Some(format!("{code}: {message}")),
            (Some(code), None) => Some((*code).to_string()),
            (None, Some(message)) => Some(message.clone()),
            (None, None) => None,
        };

        if outcome.success {
            if let Err(error) = operations::finish_webhook_delivery(
                &self.db,
                delivery.id,
                true,
                outcome.status_code.map(i32::from),
                None,
                outcome.response_snippet.as_deref(),
                false,
                None,
            )
            .await
            {
                error!(delivery_id = %delivery.id, error = %error, "could not mark webhook delivered");
            }
            let _ =
                operations::update_endpoint_delivery_status(&self.db, delivery.endpoint_id, true)
                    .await;
            info!(
                delivery_id = %delivery.id,
                endpoint_id = %delivery.endpoint_id,
                attempts = delivery.attempts,
                "webhook delivered"
            );
            return WebhookOutcome {
                delivery_id: delivery.id,
                status: "succeeded",
                attempts: delivery.attempts,
                next_attempt_in_seconds: None,
                error: None,
            };
        }

        let next_delay = if outcome.retryable {
            retry_delay_seconds(delivery.attempts)
        } else {
            None
        };
        let dead = next_delay.is_none();
        let next_attempt_at =
            next_delay.map(|seconds| Utc::now() + chrono::Duration::seconds(seconds as i64));
        if let Err(error) = operations::finish_webhook_delivery(
            &self.db,
            delivery.id,
            false,
            outcome.status_code.map(i32::from),
            combined_error.as_deref(),
            outcome.response_snippet.as_deref(),
            dead,
            next_attempt_at,
        )
        .await
        {
            error!(delivery_id = %delivery.id, error = %error, "could not record webhook failure");
        }
        let _ = operations::update_endpoint_delivery_status(&self.db, delivery.endpoint_id, false)
            .await;

        let status = if dead { "dead_letter" } else { "failed" };
        info!(
            delivery_id = %delivery.id,
            endpoint_id = %delivery.endpoint_id,
            attempts = delivery.attempts,
            retry_in = ?next_delay,
            dead,
            "webhook delivery failed"
        );
        WebhookOutcome {
            delivery_id: delivery.id,
            status,
            attempts: delivery.attempts,
            next_attempt_in_seconds: next_delay,
            error: combined_error,
        }
    }

    async fn dead_letter(
        &self,
        delivery: &operations::ClaimedDeliveryRow,
        message: &str,
    ) -> WebhookOutcome {
        if let Err(error) = operations::record_delivery_attempt(
            &self.db,
            delivery.organization_id,
            delivery.id,
            delivery.attempts,
            None,
            Some(message),
            None,
            0,
        )
        .await
        {
            error!(delivery_id = %delivery.id, error = %error, "could not record dead-letter attempt");
        }
        if let Err(error) = operations::finish_webhook_delivery(
            &self.db,
            delivery.id,
            false,
            None,
            Some(message),
            None,
            true,
            None,
        )
        .await
        {
            error!(delivery_id = %delivery.id, error = %error, "could not mark webhook dead");
        }
        let _ = operations::update_endpoint_delivery_status(&self.db, delivery.endpoint_id, false)
            .await;
        WebhookOutcome {
            delivery_id: delivery.id,
            status: "dead_letter",
            attempts: delivery.attempts,
            next_attempt_in_seconds: None,
            error: Some(message.to_string()),
        }
    }
}
