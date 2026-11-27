//! The "vlc-http" protocol: VLC's web remote, on 127.0.0.1 only, with a
//! random password per cast.

use crate::diagnose::Tail;
use castable_net::{CastError, ErrorCode, Gen, Handle, PlayerCommand, PlayerEvent};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedReceiver;

/// The remote commands for one control.
pub fn encode(ctl: &PlayerCommand) -> Vec<String> {
    match ctl {
        PlayerCommand::Play => vec!["pl_forceresume".into()],
        PlayerCommand::Pause => vec!["pl_forcepause".into()],
        PlayerCommand::Stop => vec!["pl_forcepause".into(), "seek&val=0".into()],
        PlayerCommand::Seek { to } => vec![format!("seek&val={}", to.as_secs())],
        // VLC's scale: 256 = 100%.
        PlayerCommand::SetVolume { volume: v } => vec![format!("volume&val={}", u32::from(*v) * 256 / 100)],
        PlayerCommand::SetMuted { .. } => vec![], // the web remote has no mute
        PlayerCommand::Load { .. } => vec![],     // a load starts a new player instead
    }
}

/// Where one VLC's web remote listens.
pub struct Remote {
    pub port: u16,
    pub password: String,
}

impl Remote {
    pub fn new() -> Self {
        // A free port, released for VLC to take a moment later.
        let port =
            std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).map_or(8080, |a| a.port());
        Self { port, password: uuid::Uuid::new_v4().simple().to_string() }
    }

    /// GET /requests/status.json[?command=...]; returns the JSON body.
    async fn status(&self, command: Option<&str>) -> std::io::Result<serde_json::Value> {
        let path = command.map_or("/requests/status.json".into(), |c| format!("/requests/status.json?command={c}"));
        let auth = base64(format!(":{}", self.password).as_bytes());
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port)).await?;
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {auth}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        let raw = String::from_utf8_lossy(&raw);
        let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        serde_json::from_str(body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl Default for Remote {
    fn default() -> Self {
        Self::new()
    }
}

/// One VLC's remote session: sends queued controls, polls status into events.
pub async fn session(
    gen: Gen,
    remote: Remote,
    mut controls: UnboundedReceiver<PlayerCommand>,
    handle: Handle,
    tail: Tail,
) {
    let started = Instant::now();
    let mut poll = tokio::time::interval(Duration::from_millis(500));
    let (mut loaded, mut last_progress) = (false, Instant::now());
    let mut pending: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            ctl = controls.recv() => match ctl {
                Some(ctl) => pending.extend(encode(&ctl)),
                None => return, // a new load replaced this one
            },
            _ = poll.tick() => {
                let Ok(status) = remote.status(None).await else {
                    if !loaded && started.elapsed() > Duration::from_secs(30) {
                        let error = tail.refine(CastError::with_detail(ErrorCode::PlayerUnresponsive, "VLC's web remote never answered"));
                        handle.player_event(PlayerEvent::LoadFailed { gen, error });
                        return;
                    }
                    continue; // not up yet: keep the pending controls
                };
                // The remote is up: send what queued while it started.
                for c in pending.drain(..) {
                    let _ = remote.status(Some(&c)).await;
                }
                let state = status["state"].as_str().unwrap_or("stopped");
                let duration = status["length"].as_u64().filter(|&s| s > 0).map(Duration::from_secs);
                let position = status["time"].as_u64().map(Duration::from_secs).unwrap_or_default();
                if !loaded && (state == "playing" || state == "paused") {
                    loaded = true;
                    handle.player_event(PlayerEvent::Loaded { gen, duration });
                } else if loaded && last_progress.elapsed() >= Duration::from_secs(1) {
                    last_progress = Instant::now();
                    handle.player_event(PlayerEvent::Progress { gen, position, duration });
                }
            }
        }
    }
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b":secret"), "OnNlY3JldA==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
    }

    #[test]
    fn controls_map_to_remote_commands() {
        assert_eq!(encode(&PlayerCommand::SetVolume { volume: 100 }), ["volume&val=256"]);
        assert_eq!(encode(&PlayerCommand::Seek { to: Duration::from_secs(90) }), ["seek&val=90"]);
    }
}
