//! Native display titles for agents that do not publish session titles via OSC.
//! Callers validate process identity before and after invoking this capability.

mod hermes;

use std::future::Future;

use super::tracking::LiveSessionContext;
use crate::Agent;

pub trait AgentSessionTitleProvider {
    fn session_title(
        &self,
        context: LiveSessionContext<'_>,
    ) -> impl Future<Output = Option<String>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleAgent {
    Hermes,
}

impl Agent {
    /// None preserves the terminal's native title, including activity indicators.
    pub fn session_titles(self) -> Option<TitleAgent> {
        match self {
            Self::Hermes => Some(TitleAgent::Hermes),
            _ => None,
        }
    }
}

impl AgentSessionTitleProvider for TitleAgent {
    async fn session_title(&self, context: LiveSessionContext<'_>) -> Option<String> {
        match self {
            Self::Hermes => hermes::Hermes.session_title(context).await,
        }
    }
}
