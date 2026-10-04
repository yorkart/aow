use std::sync::Arc;

use aow_operation_log::{Level, Outcome};

use super::OperationService;

struct HandleInner {
    service: OperationService,
    id: String,
}
impl Drop for HandleInner {
    fn drop(&mut self) {
        self.service.update(
            &self.id,
            "finished",
            Level::Warn,
            "执行过程已中断，结果未确认；请检查资源状态".into(),
            None,
            Some(Outcome::Interrupted),
            None,
        );
    }
}

#[derive(Clone)]
pub(crate) struct Handle {
    inner: Arc<HandleInner>,
}
impl Handle {
    pub(super) fn new(service: OperationService, id: String) -> Self {
        Self {
            inner: Arc::new(HandleInner { service, id }),
        }
    }

    pub(crate) fn progress(&self, message: impl Into<String>, completed: Option<usize>) {
        self.inner.service.update(
            &self.inner.id,
            "progress",
            Level::Info,
            message.into(),
            completed,
            None,
            None,
        );
    }
    pub(crate) fn event(&self, level: Level, message: impl Into<String>, completed: Option<usize>) {
        self.inner.service.update(
            &self.inner.id,
            "step",
            level,
            message.into(),
            completed,
            None,
            None,
        );
    }
    pub(crate) fn resource(&self, resource: String) {
        self.inner.service.update(
            &self.inner.id,
            "resource",
            Level::Info,
            "关联资源已更新".into(),
            None,
            None,
            Some(resource),
        );
    }
    pub(crate) fn finish(&self, outcome: Outcome, message: impl Into<String>) {
        let level = match outcome {
            Outcome::Succeeded => Level::Info,
            Outcome::Failed => Level::Error,
            _ => Level::Warn,
        };
        self.inner.service.update(
            &self.inner.id,
            "finished",
            level,
            message.into(),
            None,
            Some(outcome),
            None,
        );
    }
}
