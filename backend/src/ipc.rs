//! Local TCP server (127.0.0.1:ipcPort) for okru-tui.
//! - receives `changed` → asks the reloader to re-read SQLite immediately
//! - broadcasts backend events (reloads, JOIN/PART, worker sync) to every connected TUI

use std::sync::Arc;

use anyhow::{Context, Result};
use okru_tui::ipc::{encode, ClientMsg, ServerMsg};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};

use crate::registry::SnapshotRx;

/// Reload request: `Some(client)` when a TUI asked for it.
pub type ReloadTx = mpsc::UnboundedSender<Option<String>>;

/// Fan-out of backend events to connected TUIs. Sending with nobody connected is a no-op.
pub struct Hub {
    events: broadcast::Sender<ServerMsg>,
}

impl Hub {
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self { events })
    }

    pub fn emit(&self, msg: ServerMsg) {
        let _ = self.events.send(msg);
    }
}

pub async fn serve(port: u16, hub: Arc<Hub>, reload: ReloadTx, rx: SnapshotRx) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("IPC bind 127.0.0.1:{port} (¿otro okru-backend corriendo?)"))?;
    tracing::info!("IPC TUI escuchando en 127.0.0.1:{port}");
    loop {
        let (stream, _) = listener.accept().await?;
        let (hub, reload, rx) = (Arc::clone(&hub), reload.clone(), rx.clone());
        tokio::spawn(async move {
            if let Err(e) = handle(stream, &hub, &reload, &rx).await {
                tracing::debug!("IPC conexión cerrada: {e:#}");
            }
        });
    }
}

async fn handle(stream: TcpStream, hub: &Hub, reload: &ReloadTx, rx: &SnapshotRx) -> Result<()> {
    stream.set_nodelay(true).ok();
    let (reader, mut writer) = stream.into_split();
    // Subscribe before the welcome so no event is missed in between.
    let mut events = hub.events.subscribe();

    let welcome = {
        let snap = rx.borrow();
        let mut channels: Vec<String> = snap.channels().into_iter().collect();
        channels.sort();
        ServerMsg::Welcome { users: snap.total, active: snap.len(), channels }
    };
    writer.write_all(encode(&welcome).as_bytes()).await?;

    let mut lines = BufReader::new(reader).lines();
    let mut client = String::from("tui");
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                match serde_json::from_str::<ClientMsg>(&line) {
                    Ok(ClientMsg::Hello { client: name }) => {
                        tracing::info!("IPC: {name} conectado");
                        client = name;
                    }
                    Ok(ClientMsg::Changed) => {
                        tracing::info!("IPC: {client} avisó cambios → recargando DB");
                        let _ = reload.send(Some(client.clone()));
                    }
                    Err(e) => tracing::warn!("IPC: mensaje inválido de {client}: {e}"),
                }
            }
            event = events.recv() => match event {
                Ok(msg) => writer.write_all(encode(&msg).as_bytes()).await?,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
    tracing::info!("IPC: {client} desconectado");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Snapshot;
    use tokio::io::AsyncBufReadExt;
    use tokio::sync::watch;

    #[tokio::test]
    async fn changed_triggers_reload_and_events_reach_client() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let hub = Hub::new();
        let (reload_tx, mut reload_rx) = mpsc::unbounded_channel();
        let (_snap_tx, snap_rx) = watch::channel(Arc::new(Snapshot::default()));
        tokio::spawn(serve(port, Arc::clone(&hub), reload_tx, snap_rx));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, mut w) = stream.into_split();
        let mut lines = BufReader::new(r).lines();

        let welcome: ServerMsg = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert!(matches!(welcome, ServerMsg::Welcome { active: 0, .. }));

        w.write_all(encode(&ClientMsg::Hello { client: "tui-test".into() }).as_bytes()).await.unwrap();
        w.write_all(encode(&ClientMsg::Changed).as_bytes()).await.unwrap();
        assert_eq!(reload_rx.recv().await.unwrap(), Some("tui-test".into()));

        hub.emit(ServerMsg::Joined { channel: "perghor_adamia".into() });
        let event: ServerMsg = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(event, ServerMsg::Joined { channel: "perghor_adamia".into() });
    }
}
