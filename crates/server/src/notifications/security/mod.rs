//! Bounded security delivery queue. Authentication logs are persisted before
//! enqueueing; each provider's delivery result is appended to the same history.
use std::{sync::Mutex, time::Duration};

use aow_im::{ImKind, ImProvider, Message, Provider};
use aow_operation_log::{Level, Outcome, Record};
use tokio::sync::mpsc;

use crate::operations::OperationService;

struct Delivery {
    message: Message,
    record: Record,
    operations: OperationService,
    providers: Vec<(ImKind, Provider)>,
}

pub(super) struct Dispatcher {
    sender: mpsc::Sender<Delivery>,
    receiver: Mutex<Option<mpsc::Receiver<Delivery>>>,
}

impl Default for Dispatcher {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel(super::QUEUE_CAPACITY);
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
        }
    }
}

impl Dispatcher {
    pub(super) async fn dispatch(
        &self,
        message: Message,
        record: Record,
        operations: OperationService,
        providers: Vec<(ImKind, Provider)>,
    ) {
        if providers.is_empty() {
            return;
        }
        if let Some(mut receiver) = self.receiver.lock().unwrap().take() {
            tokio::spawn(async move {
                while let Some(delivery) = receiver.recv().await {
                    // One unavailable provider must not prevent another from sending.
                    futures_util::future::join_all(delivery.providers.iter().map(
                        |(kind, provider)| {
                            deliver(
                                provider,
                                *kind,
                                &delivery.message,
                                &delivery.record,
                                &delivery.operations,
                            )
                        },
                    ))
                    .await;
                }
            });
        }
        // Apply backpressure, never deduplicate or drop when the queue is full.
        if let Err(error) = self
            .sender
            .send(Delivery {
                message,
                record,
                operations,
                providers,
            })
            .await
        {
            let delivery = error.0;
            for (kind, _) in delivery.providers {
                delivery_result(
                    &delivery.operations,
                    delivery.record.clone(),
                    kind,
                    "queue_closed",
                )
                .await;
            }
        }
    }
}

async fn deliver(
    provider: &impl ImProvider,
    kind: ImKind,
    message: &Message,
    record: &Record,
    operations: &OperationService,
) {
    let result = match tokio::time::timeout(
        Duration::from_secs(30),
        provider.send(message, &record.operation_id),
    )
    .await
    {
        Ok(Ok(())) => "sent",
        Ok(Err(_)) => "delivery_failed",
        Err(_) => "delivery_timeout",
    };
    // Provider errors can contain credentials or upstream payloads. Persist
    // only a stable outcome, never their raw error text.
    delivery_result(operations, record.clone(), kind, result).await;
}

async fn delivery_result(
    operations: &OperationService,
    mut record: Record,
    provider: ImKind,
    result: &str,
) {
    let success = result == "sent";
    record.timestamp = chrono::Utc::now().to_rfc3339();
    record.kind = "auth.notification".into();
    record.event = "notification_delivery".into();
    record.title = "认证通知投递".into();
    record.message = serde_json::json!({"provider": provider, "result": result}).to_string();
    record.level = if success { Level::Info } else { Level::Error };
    record.outcome = Some(if success {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    });
    if !success {
        tracing::warn!(event_id = %record.operation_id, ?provider, result, "authentication notification failed");
    }
    operations.record(record).await;
}

#[cfg(test)]
mod tests;
