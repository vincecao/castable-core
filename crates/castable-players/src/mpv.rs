//! The "mpv-ipc" protocol: mpv's JSON IPC socket (mpv, IINA, other mpv front-ends).

use castable_net::{Gen, Handle, PlayerCommand, PlayerEvent};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

/// The IPC messages for one control.
pub fn encode(ctl: &PlayerCommand) -> Vec<String> {
    let set = |prop: &str, v: &str| format!(r#"{{"command":["set_property","{prop}",{v}]}}"#);
    match ctl {
        PlayerCommand::Play => vec![set("pause", "false")],
        PlayerCommand::Pause => vec![set("pause", "true")],
        PlayerCommand::Stop => vec![set("pause", "true"), r#"{"command":["seek",0,"absolute"]}"#.into()],
        PlayerCommand::Seek { to } => vec![format!(r#"{{"command":["seek",{},"absolute"]}}"#, to.as_secs_f64())],
        PlayerCommand::SetVolume { volume: v } => vec![set("volume", &v.to_string())],
        PlayerCommand::SetMuted { muted: m } => vec![set("mute", &m.to_string())],
        PlayerCommand::Load { .. } => vec![], // a load starts a new player instead
    }
}

/// Where a player's IPC endpoint lives: a Unix socket, or a named pipe on Windows.
pub fn endpoint(session_id: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\castable-{session_id}"))
    } else {
        std::env::temp_dir().join(format!("castable-{session_id}.sock"))
    }
}

#[cfg(unix)]
async fn connect(path: &std::path::Path) -> std::io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(path).await
}

#[cfg(windows)]
async fn connect(path: &std::path::Path) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(path)
}

/// One player's IPC conversation: sends queued commands (they wait until the
/// endpoint is up, so nothing sent during start-up is lost) and turns mpv's
/// events into PlayerEvents.
pub async fn session(gen: Gen, endpoint: PathBuf, mut controls: UnboundedReceiver<PlayerCommand>, handle: Handle) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let started = Instant::now();
    let stream = loop {
        match connect(&endpoint).await {
            Ok(s) => break s,
            Err(_) if started.elapsed() < Duration::from_secs(20) => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(e) => {
                handle.player_event(PlayerEvent::LoadFailed { gen, reason: format!("the player didn't start: {e}") });
                return;
            }
        }
    };
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    for watch in
        [r#"{"command":["observe_property",1,"duration"]}"#, r#"{"command":["observe_property",2,"time-pos"]}"#]
    {
        let _ = write.write_all(format!("{watch}\n").as_bytes()).await;
    }
    let (mut loaded, mut duration, mut last_progress) = (false, None, Instant::now());
    loop {
        tokio::select! {
            ctl = controls.recv() => match ctl {
                Some(ctl) => {
                    for msg in encode(&ctl) {
                        let _ = write.write_all(format!("{msg}\n").as_bytes()).await;
                    }
                }
                None => return, // a new load replaced this one
            },
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { return };
                let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if let Some(event) = read_event(gen, &msg, &mut loaded, &mut duration, &mut last_progress) {
                    handle.player_event(event);
                }
            }
        }
    }
}

/// Turns one mpv message into an event, if it is one.
fn read_event(
    gen: Gen,
    msg: &serde_json::Value,
    loaded: &mut bool,
    duration: &mut Option<Duration>,
    last_progress: &mut Instant,
) -> Option<PlayerEvent> {
    let secs = |v: &serde_json::Value| v.as_f64().filter(|s| s.is_finite() && *s >= 0.0).map(Duration::from_secs_f64);
    match (msg["event"].as_str(), msg["name"].as_str()) {
        (Some("file-loaded"), _) => {
            *loaded = true;
            Some(PlayerEvent::Loaded { gen, duration: *duration })
        }
        (Some("property-change"), Some("duration")) => {
            *duration = secs(&msg["data"]);
            None
        }
        (Some("property-change"), Some("time-pos")) if *loaded && last_progress.elapsed() >= Duration::from_secs(1) => {
            let position = secs(&msg["data"])?;
            *last_progress = Instant::now();
            Some(PlayerEvent::Progress { gen, position, duration: *duration })
        }
        (Some("end-file"), _) => match msg["reason"].as_str() {
            Some("eof") => Some(PlayerEvent::Ended { gen }),
            Some("error") => Some(PlayerEvent::LoadFailed {
                gen,
                reason: msg["file_error"].as_str().unwrap_or("the player couldn't play it").to_string(),
            }),
            _ => None, // stopped or replaced by us
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_follow_mpv() {
        let (mut loaded, mut duration, mut last) = (false, None, Instant::now() - Duration::from_secs(5));
        let mut read = |m| read_event(Gen(1), &m, &mut loaded, &mut duration, &mut last);
        assert_eq!(read(json!({"event":"property-change","name":"duration","data":6835.0})), None);
        assert_eq!(
            read(json!({"event":"file-loaded"})),
            Some(PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(6835)) })
        );
        assert!(matches!(
            read(json!({"event":"property-change","name":"time-pos","data":30.0})),
            Some(PlayerEvent::Progress { .. })
        ));
        assert_eq!(read(json!({"event":"end-file","reason":"eof"})), Some(PlayerEvent::Ended { gen: Gen(1) }));
        assert_eq!(read(json!({"event":"end-file","reason":"stop"})), None);
    }

    #[test]
    fn stop_pauses_and_rewinds() {
        assert_eq!(encode(&PlayerCommand::Stop).len(), 2);
        assert_eq!(
            encode(&PlayerCommand::Seek { to: Duration::from_millis(1500) }),
            [r#"{"command":["seek",1.5,"absolute"]}"#]
        );
    }
}
