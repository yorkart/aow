use super::*;

use aow_im::ImProvider;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    notifications::{Channel, messages},
    terminal::notifications::TaskStopNotification,
};
use std::sync::atomic::Ordering;

impl NotificationManager {
    pub(crate) fn start(&self) {
        self.inner.started.store(true, Ordering::Relaxed);
        for configured in &self.inner.state.lock().unwrap().providers {
            configured.provider.start();
        }
        let Some(mut receiver) = self.inner.receiver.lock().unwrap().take() else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            while let Some(delivery) = receiver.recv().await {
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let providers: Vec<_> = {
                    let state = inner.state.lock().unwrap();
                    let settings = &state.document.notifications.agent_task_completed;
                    // Settings changes invalidate queued deliveries, including changed credentials.
                    if state.revision != delivery.revision || !settings.enabled {
                        continue;
                    }
                    settings
                        .channels
                        .iter()
                        .filter_map(|channel| channel.provider())
                        .filter_map(|kind| {
                            state
                                .providers
                                .iter()
                                .find(|provider| provider.config.kind() == kind)
                        })
                        .map(|provider| (provider.config.kind(), provider.provider.clone()))
                        .collect()
                };
                drop(inner);
                for (kind, provider) in providers {
                    if let Err(error) = provider
                        .send(&messages::task_completed(&delivery.event), &delivery.id)
                        .await
                    {
                        tracing::warn!(provider = ?kind, session_id = %delivery.event.session_id, %error, "agent notification delivery failed");
                    }
                }
            }
        });
    }

    pub(crate) fn dispatch(
        &self,
        mut event: TaskStopNotification,
        page: &broadcast::Sender<TaskStopNotification>,
    ) {
        let state = self.inner.state.lock().unwrap();
        let settings = &state.document.notifications.agent_task_completed;
        if !settings.enabled {
            return;
        }
        if let Ok(base) = reqwest::Url::parse(&state.document.notifications.public_base_url) {
            for source in &mut event.sources {
                let mut url = base.clone();
                if let Ok(mut segments) = url.path_segments_mut() {
                    segments
                        .pop_if_empty()
                        .extend(["aow", "tabs", "terminal", &source.tab_id]);
                }
                source.tab_url = Some(url.into());
            }
        }
        if settings.channels.contains(&Channel::Page) {
            let _ = page.send(event.clone());
        }
        if settings
            .channels
            .iter()
            .any(|channel| channel.provider().is_some())
        {
            let delivery = Delivery {
                event,
                revision: state.revision,
                id: Uuid::new_v4().to_string(),
            };
            if self.inner.sender.try_send(delivery).is_err() {
                tracing::warn!("IM notification queue full or closed; delivery dropped");
            }
        }
    }
}
