use anyhow::{Result, bail, ensure};
use aow_acp_core::{AcpService, ExecutionEnvironment, SessionCreation};
use std::{path::PathBuf, time::Duration};
#[derive(Clone)]
pub struct AcpProber {
    service: AcpService,
    stopping: tokio::sync::watch::Receiver<bool>,
}
struct Session(AcpService, Option<String>);
impl Drop for Session {
    fn drop(&mut self) {
        let service = self.0.clone();
        let Some(id) = self.1.take() else { return };
        aow_i18n::spawn(async move {
            let _ = service.delete_session(&id).await;
        });
    }
}
impl AcpProber {
    pub(crate) fn new(service: AcpService, stopping: tokio::sync::watch::Receiver<bool>) -> Self {
        Self { service, stopping }
    }
    pub async fn probe(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        execution_environment: ExecutionEnvironment,
    ) -> Result<()> {
        ensure!(!*self.stopping.borrow(), "Node is stopping");
        let session = self
            .service
            .prepare_session_with_options(
                agent_id,
                cwd,
                SessionCreation {
                    execution_environment: Some(execution_environment),
                    ..Default::default()
                },
            )
            .await?;
        let mut guard = Session(self.service.clone(), Some(session.thread_id.clone()));
        let mut stopping = self.stopping.clone();
        let result=tokio::time::timeout(Duration::from_secs(60),async{
            self.service.resume(&session.thread_id).await?;
            self.service.start_prompt(&session.thread_id,serde_json::from_value(serde_json::json!([{"type":"text","text":"ping"}]))?)?;
            loop {
                let snapshot=self.service.snapshot(&session.thread_id)?;
                ensure!(snapshot.permissions.is_empty(),"Runtime requested permission during connection test");
                if let Some(error)=&snapshot.error {bail!("{error}");}
                if snapshot.status=="idle" {
                    ensure!(snapshot.entries.iter().any(|entry|entry.kind=="assistant"&&entry.content["text"].as_str().is_some_and(|text|!text.trim().is_empty())),"Runtime did not return a response");return Ok(());
                }
                tokio::select!{_=tokio::time::sleep(Duration::from_millis(40))=>{},_=stopping.changed()=>bail!("Node is stopping")}
            }
        }).await;
        self.service
            .delete_session(guard.1.as_deref().unwrap())
            .await?;
        guard.1 = None;
        drop(guard);
        result?
    }
}
