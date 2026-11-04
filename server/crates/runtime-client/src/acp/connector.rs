use crate::RuntimeClient;
use anyhow::Result;
use aow_acp::{AcpConnector, AdapterTransport, ExecutionEnvironment};
use futures::{FutureExt, SinkExt, StreamExt, channel::mpsc};
use std::{io, path::Path, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub(super) struct Connector(pub RuntimeClient);
struct Cleanup {
    runtime: RuntimeClient,
    id: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let runtime = self.runtime.clone();
        let id = self.id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = runtime.delete(&format!("/v1/acp/instances/{id}")).await;
            });
        }
    }
}
#[async_trait::async_trait]
impl AcpConnector for Connector {
    async fn connect(
        &self,
        agent_id: &str,
        cwd: &Path,
        execution: Option<&ExecutionEnvironment>,
        environment: &std::collections::BTreeMap<String, String>,
        connection_id: &str,
    ) -> Result<AdapterTransport> {
        self.0.acp_ready().await?;
        // Admission owns the request until it completes. Cancellation by a caller must
        // not issue DELETE before the node has admitted the corresponding PUT.
        let runtime = self.0.clone();
        let id = connection_id.to_owned();
        let spec = crate::AcpInstanceSpec {
            agent_id: agent_id.into(),
            cwd: cwd.into(),
            execution_environment: execution.cloned(),
            environment: environment.clone(),
        };
        let (ready, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let cleanup = Cleanup {
                runtime: runtime.clone(),
                id: id.clone(),
            };
            let result: Result<_> = async {
                runtime.create_acp_instance(&id, &spec).await?;
                let socket = runtime.attach_acp(&id).await?;
                Ok((socket, cleanup))
            }
            .await;
            // A dropped receiver drops the socket and cleanup guard together.
            let _ = ready.send(result);
        });
        let (mut socket, cleanup) = receiver.await??;
        let (mut incoming_tx, incoming) = mpsc::channel(16);
        let (outgoing, outgoing_rx) = mpsc::channel::<String>(16);
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let lifetime=async move {
            let _cleanup=cleanup;
            let mut outgoing_rx=outgoing_rx;
            let result:Result<()>=async {
                loop {
                    tokio::select! {
                        _=stopped.cancelled()=>break,
                        frame=socket.next()=>match frame {
                            Some(Ok(Message::Text(text)))=>tokio::time::timeout(Duration::from_secs(5),incoming_tx.send(Ok(text.to_string()))).await??,
                            Some(Ok(Message::Ping(_) | Message::Pong(_)))=>{},
                            Some(Ok(Message::Close(_)))|None=>break,
                            Some(Err(error))=>return Err(error.into()),
                            _=>{},
                        },
                        text=outgoing_rx.next()=>{
                            let Some(text)=text else{break;};
                            tokio::time::timeout(Duration::from_secs(5),socket.send(Message::Text(text.into()))).await??;
                        }
                    }
                }
                Ok(())
            }.await;
            if let Err(error)=result {let _=incoming_tx.send(Err(io::Error::other(error.to_string()))).await;}
            RuntimeClient::close_socket(socket).await;
        }.boxed();
        Ok(AdapterTransport {
            incoming: incoming.boxed(),
            outgoing: Box::pin(outgoing.sink_map_err(|error| io::Error::other(error.to_string()))),
            stop,
            lifetime,
        })
    }
}
