//! TCP client to okru-backend (`127.0.0.1:ipcPort`).
//! Non-blocking for the UI: a reader thread forwards server events through a channel,
//! and the link reconnects on its own while the backend is down.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use okru_tui::ipc::{encode, ClientMsg, ServerMsg};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const RECONNECT_EVERY: Duration = Duration::from_secs(2);

pub enum LinkEvent {
    Connected,
    Disconnected,
    Message(ServerMsg),
}

pub struct Link {
    pub port: u16,
    /// Identifies this TUI in `reloaded.by` (ignore our own reloads).
    pub client_id: String,
    writer: Option<TcpStream>,
    /// Connection generation: events from older reader threads are dropped.
    generation: u64,
    events_tx: Sender<(u64, LinkEvent)>,
    events_rx: Receiver<(u64, LinkEvent)>,
    last_attempt: Option<Instant>,
    enabled: bool,
}

impl Link {
    pub fn new(port: u16) -> Self {
        let mut link = Self::build(port, true);
        link.try_connect();
        link
    }

    /// Never connects (tests).
    #[cfg(test)]
    pub fn offline() -> Self {
        Self::build(0, false)
    }

    fn build(port: u16, enabled: bool) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            port,
            client_id: format!("okru-tui#{}", std::process::id()),
            writer: None,
            generation: 0,
            events_tx,
            events_rx,
            last_attempt: None,
            enabled,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.writer.is_some()
    }

    /// Drains pending events and reconnects when due. Call every UI tick.
    pub fn poll(&mut self) -> Vec<LinkEvent> {
        if self.writer.is_none() && self.last_attempt.is_none_or(|t| t.elapsed() >= RECONNECT_EVERY) {
            self.try_connect();
        }
        let current = self.generation;
        let events: Vec<LinkEvent> = self
            .events_rx
            .try_iter()
            .filter(|(generation, _)| *generation == current)
            .map(|(_, event)| event)
            .collect();
        if events.iter().any(|e| matches!(e, LinkEvent::Disconnected)) {
            self.writer = None;
        }
        events
    }

    /// Tell the backend the DB changed. Returns `false` if it is not reachable.
    pub fn notify_changed(&mut self) -> bool {
        self.send(&ClientMsg::Changed)
    }

    fn send(&mut self, msg: &ClientMsg) -> bool {
        let Some(writer) = self.writer.as_mut() else {
            return false;
        };
        if writer.write_all(encode(msg).as_bytes()).is_ok() {
            return true;
        }
        self.writer = None;
        let _ = self.events_tx.send((self.generation, LinkEvent::Disconnected));
        false
    }

    fn try_connect(&mut self) {
        if !self.enabled {
            return;
        }
        self.last_attempt = Some(Instant::now());
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let Ok(stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
            return;
        };
        stream.set_nodelay(true).ok();
        let Ok(reader) = stream.try_clone() else {
            return;
        };

        self.generation += 1;
        let generation = self.generation;
        let tx = self.events_tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if let Ok(msg) = serde_json::from_str::<ServerMsg>(&line) {
                    if tx.send((generation, LinkEvent::Message(msg))).is_err() {
                        return;
                    }
                }
            }
            let _ = tx.send((generation, LinkEvent::Disconnected));
        });

        self.writer = Some(stream);
        let _ = self.events_tx.send((generation, LinkEvent::Connected));
        let hello = ClientMsg::Hello { client: self.client_id.clone() };
        self.send(&hello);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn wait_for(link: &mut Link, pred: impl Fn(&LinkEvent) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if link.poll().iter().any(&pred) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn talks_to_a_backend_and_detects_disconnect() {
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        let backend = std::thread::spawn(move || {
            let (stream, _) = server.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let hello: ClientMsg = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert!(matches!(hello, ClientMsg::Hello { .. }));
            let changed: ClientMsg = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(changed, ClientMsg::Changed);
            writer
                .write_all(encode(&ServerMsg::Joined { channel: "perghor_adamia".into() }).as_bytes())
                .unwrap();
            // dropping the socket = backend went away
        });

        let mut link = Link::new(port);
        assert!(link.is_connected());
        assert!(link.notify_changed());
        assert!(wait_for(&mut link, |e| matches!(
            e,
            LinkEvent::Message(ServerMsg::Joined { channel }) if channel == "perghor_adamia"
        )));
        backend.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while link.is_connected() && Instant::now() < deadline {
            link.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!link.is_connected());
    }

    #[test]
    fn offline_link_reports_not_delivered() {
        let mut link = Link::offline();
        assert!(!link.notify_changed());
        assert!(link.poll().is_empty());
    }
}
