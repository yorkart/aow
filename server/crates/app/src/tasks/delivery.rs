//! Task persistence operations used by message delivery and the user inbox.
use super::TaskService;
use anyhow::Result;
use aow_db::tasks::{DeliveryState, InboxPage, MessageDelivery, TaskEvent, TaskRecord, TaskStatus};
impl TaskService {
    pub(crate) fn message_for_prompt(
        &self,
        task: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<MessageDelivery>> {
        self.inner.tasks.message_for_prompt(task, thread, id)
    }
    pub(crate) fn list(&self) -> Result<Vec<TaskRecord>> {
        self.inner.tasks.list()
    }
    pub(crate) fn status(&self, id: &str) -> Result<Option<TaskStatus>> {
        self.inner.tasks.status(id)
    }
    pub(crate) fn event(&self, id: &str) -> Result<Option<TaskEvent>> {
        self.inner.tasks.event(id)
    }
    pub(crate) fn pending_messages(&self) -> Result<Vec<MessageDelivery>> {
        self.inner.tasks.pending_messages()
    }
    pub(crate) fn update_message(
        &self,
        id: &str,
        state: DeliveryState,
        error: Option<String>,
        retry_at: i64,
    ) -> Result<()> {
        self.inner.tasks.update_message(id, state, error, retry_at)
    }
    pub(crate) fn inbox(
        &self,
        project: Option<&str>,
        unread: bool,
        archived: bool,
        offset: u32,
        limit: u32,
    ) -> Result<InboxPage> {
        self.inner
            .tasks
            .inbox(project, unread, archived, offset, limit)
    }
    pub(crate) fn update_inbox(
        &self,
        id: &str,
        read: Option<bool>,
        archived: Option<bool>,
    ) -> Result<()> {
        self.inner.tasks.update_inbox(id, read, archived)
    }
    pub(crate) fn retry_delivery(&self, event: &str) -> Result<()> {
        self.inner.tasks.retry_delivery(event)
    }
}
