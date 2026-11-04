use super::*;
use axum::extract::{WebSocketUpgrade, ws::Message};
use futures_util::StreamExt;
use std::time::{Duration, Instant};
pub(super) async fn subscribe(
    State(state): State<WorkspaceState>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |mut socket| async move {
        let mut stopping=state.stopping.clone();
        let watcher=match crate::git::GitWatcher::new(Vec::new()){Ok(watcher)=>watcher,Err(_)=>return};
        let mut changes=watcher.subscribe();
        let mut heartbeat=tokio::time::interval(Duration::from_secs(15));
        let mut received=Instant::now();
        loop {
            if *stopping.borrow(){return;}
            tokio::select! {
                _=stopping.changed()=>return,
                message=socket.next()=>match message {
                    Some(Ok(Message::Text(text)))=>{
                        received=Instant::now();
                        let Ok(roots)=serde_json::from_str::<Vec<std::path::PathBuf>>(&text) else{return};
                        if roots.len()>1024||watcher.set_repositories(roots).is_err(){return;}
                    },
                    Some(Ok(Message::Ping(data)))=>{
                        received=Instant::now();
                        if !matches!(tokio::time::timeout(Duration::from_secs(5),socket.send(Message::Pong(data))).await,Ok(Ok(()))){return;}
                    },
                    Some(Ok(Message::Pong(_)))=>{received=Instant::now();},
                    _=>return,
                },
                changed=changes.changed()=>{
                    if changed.is_err(){return;}
                    let next=changes.borrow_and_update().clone();
                    let value=serde_json::json!({"revision":next.revision,"worktrees":next.worktrees,"repositories":next.repositories});
                    if !matches!(tokio::time::timeout(Duration::from_secs(5),socket.send(Message::Text(value.to_string().into()))).await,Ok(Ok(()))){return;}
                },
                _=heartbeat.tick()=>{
                    if received.elapsed()>Duration::from_secs(45){return;}
                    if !matches!(tokio::time::timeout(Duration::from_secs(5),socket.send(Message::Ping(Vec::new().into()))).await,Ok(Ok(()))){return;}
                },
            }
        }
    })
}
