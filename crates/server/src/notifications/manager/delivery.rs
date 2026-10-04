use super::*;

use aow_im::{ImKind, ImProvider, Provider};

use crate::notifications::{AutomationFailureNotification, messages};

impl NotificationManager {
    /// Automation preferences belong to the run snapshot, independently of
    /// interactive Agent completion preferences. False means no local bot.
    pub(crate) async fn send_automation_failure(
        &self,
        run: aow_automations::Run,
        channel: aow_automations::FailureNotification,
        delivery_id: &str,
    ) -> anyhow::Result<bool> {
        let Some((provider, event)) = self.automation_delivery(run, channel) else {
            return Ok(false);
        };
        provider
            .send(&messages::automation_failure(&event), delivery_id)
            .await?;
        Ok(true)
    }

    pub(in crate::notifications) fn automation_delivery(
        &self,
        run: aow_automations::Run,
        channel: aow_automations::FailureNotification,
    ) -> Option<(Provider, AutomationFailureNotification)> {
        let (provider, base_url) = {
            let state = self.inner.state.lock().unwrap();
            (
                state
                    .providers
                    .iter()
                    .find(|provider| {
                        provider.config.kind()
                            == match channel {
                                aow_automations::FailureNotification::Feishu => ImKind::Feishu,
                                aow_automations::FailureNotification::Wechat => ImKind::Wechat,
                            }
                    })
                    .map(|provider| provider.provider.clone()),
                state.document.notifications.public_base_url.clone(),
            )
        };
        let provider = provider?;
        let run_url = reqwest::Url::parse(&base_url).ok().and_then(|mut url| {
            url.path_segments_mut().ok()?.pop_if_empty().extend([
                "aow",
                "tabs",
                "automation",
                &run.task_id,
                "runs",
                &run.id,
            ]);
            Some(url.into())
        });
        Some((provider, AutomationFailureNotification { run, run_url }))
    }

    /// Security events use every configured IM provider, independently of
    /// agent-completion preferences. Snapshot the recipients for this event.
    pub(crate) async fn notify_security(
        &self,
        message: aow_im::Message,
        record: aow_operation_log::Record,
        operations: crate::operations::OperationService,
    ) {
        let providers = self.security_providers();
        self.inner
            .security
            .dispatch(message, record, operations, providers)
            .await;
    }

    fn security_providers(&self) -> Vec<(ImKind, Provider)> {
        self.inner
            .state
            .lock()
            .unwrap()
            .providers
            .iter()
            .map(|entry| (entry.config.kind(), entry.provider.clone()))
            .collect()
    }
}
