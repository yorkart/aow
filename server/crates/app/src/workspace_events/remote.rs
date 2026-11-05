use super::*;
use aow_runtime_client::WorkspaceChanges;
use futures_util::{SinkExt, StreamExt};
use std::path::Path;
use tokio_tungstenite::tungstenite::Message;
pub(super) async fn run(aow: AowManager, changes: watch::Sender<Snapshot>, reconcile: Arc<Notify>) {
    let Ok(runtime) = aow.agent_node().await else {
        return;
    };
    loop {
        match runtime.watch_workspaces().await {
            Ok(mut socket) => {
                let Ok(roots) = aow.repository_roots() else {
                    return;
                };
                if socket
                    .send(Message::Text(serde_json::to_string(&roots).unwrap().into()))
                    .await
                    .is_err()
                {
                    continue;
                }
                let mut previous = WorkspaceChanges::default();
                let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
                let mut received = std::time::Instant::now();
                loop {
                    tokio::select! {
                        _=reconcile.notified()=>{
                            if let Ok(roots)=aow.repository_roots() && socket.send(Message::Text(serde_json::to_string(&roots).unwrap().into())).await.is_err(){break;}
                        },
                        message=socket.next()=>match message {
                            Some(Ok(Message::Text(text)))=>{
                                received=std::time::Instant::now();
                                let Ok(next)=serde_json::from_str::<WorkspaceChanges>(&text)else{break};
                                changes.send_modify(|snapshot|{
                                    snapshot.revision+=1;
                                    if previous.worktrees!=next.worktrees{snapshot.projects=snapshot.revision;}
                                    snapshot.repositories.retain(|root,_|next.repositories.contains_key(Path::new(root)));
                                    for (root,revision) in &next.repositories {
                                        if previous.repositories.get(root)!=Some(revision){snapshot.repositories.insert(root.to_string_lossy().into_owned(),snapshot.revision);}
                                    }
                                });
                                previous=next;
                            },
                            Some(Ok(Message::Ping(data)))=>{received=std::time::Instant::now();if socket.send(Message::Pong(data)).await.is_err(){break;}},
                            Some(Ok(Message::Pong(_)))=>{received=std::time::Instant::now();},_=>break,
                        },
                        _=heartbeat.tick()=>{if received.elapsed()>std::time::Duration::from_secs(45){break;}},
                    }
                }
            }
            Err(error) => tracing::warn!(%error,"workspace node watch disconnected"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
