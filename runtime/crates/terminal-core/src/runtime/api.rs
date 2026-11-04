//! Public terminal operations; registry and process handles stay private.

use super::*;
use aow_runtime_contract::agents::{TerminalAgentProgress, TerminalAgentRequest};
use futures_util::Stream;

/// A cloneable handle to the terminals owned by a [`TerminalRuntimeHost`].
#[derive(Clone)]
pub struct TerminalManager {
    state: DaemonState,
}

impl TerminalManager {
    pub(super) fn new(state: DaemonState) -> Self {
        Self { state }
    }

    pub fn health(&self) -> TerminaldHealth {
        self.state.health()
    }

    pub fn list(&self) -> Result<TerminalRuntimeList, TerminalError> {
        self.state.list()
    }

    pub fn get(&self, id: &str) -> Result<TerminalRuntime, TerminalError> {
        self.state.get(id)
    }

    pub async fn create(
        &self,
        id: String,
        spec: TerminalRuntimeSpec,
    ) -> Result<CreateOutcome, TerminalError> {
        self.state.create(id, spec).await
    }

    pub async fn delete(&self, id: &str) -> Result<(), TerminalError> {
        self.state.delete(id).await
    }

    pub fn screen(&self, id: &str) -> Result<Option<aow_protocol::TerminalScreen>, TerminalError> {
        self.state.screen(id)
    }

    pub async fn agents(&self) -> Result<TerminalAgentList, TerminalError> {
        TerminalObserver {
            state: self.state.clone(),
        }
        .agents()
        .await
    }

    pub async fn admit_agent_operation(
        &self,
        id: &str,
        request: TerminalAgentRequest,
    ) -> Result<TerminalAgentProgress, TerminalError> {
        agent_operations::admit(self.state.clone(), id, request).await
    }

    pub fn agent_operation_status(
        &self,
        id: &str,
        request_id: &str,
    ) -> Result<TerminalAgentProgress, TerminalError> {
        agent_operations::status(&self.state, id, request_id)
    }

    pub fn attach(
        &self,
        id: &str,
        options: AttachOptions,
    ) -> Result<TerminalConnection, TerminalError> {
        let mut inner = self.state.attach(
            id,
            options.epoch.as_deref(),
            options.after,
            options.vt_snapshot,
        )?;
        inner.gzip_snapshot = options.gzip_snapshot;
        Ok(TerminalConnection {
            inner,
            controlled: options.controlled,
            observe: options.observe,
        })
    }
}

/// Resume position and capabilities negotiated by the caller.
#[derive(Debug, Default)]
pub struct AttachOptions {
    pub epoch: Option<String>,
    pub after: Option<u64>,
    pub vt_snapshot: bool,
    pub gzip_snapshot: bool,
    /// Wait for an explicit claim instead of immediately taking control.
    pub controlled: bool,
    /// Observe the current controller's output when an explicit claim must wait.
    pub observe: bool,
}

/// An attachment with private ownership and replay state.
pub struct TerminalConnection {
    inner: RuntimeConnection,
    controlled: bool,
    observe: bool,
}

impl TerminalConnection {
    /// Run until input ends, delivery fails, or the terminal finishes.
    ///
    /// The output sink must flush to its destination before reporting completion:
    /// controller hand-off is ordered against delivery, including acknowledgements.
    /// An unbounded channel in front of the actual destination breaks that guarantee.
    pub async fn run<S, R>(self, sender: S, receiver: R)
    where
        S: Sink<TerminalEvent> + Unpin,
        R: Stream<Item = TerminalInput> + Unpin,
    {
        attachment::run_attachment(sender, receiver, self.inner, self.controlled, self.observe)
            .await;
    }
}
