//! The "mpv-ipc" protocol: mpv's JSON IPC socket (mpv, IINA, other mpv front-ends).

use crate::diagnose::{classify, Tail};
use castable_net::{CastError, ErrorCode, Gen, Handle, MediaInfo, PlayerCommand, PlayerEvent};
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
pub async fn session(
    gen: Gen,
    endpoint: PathBuf,
    mut controls: UnboundedReceiver<PlayerCommand>,
    handle: Handle,
    tail: Tail,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let started = Instant::now();
    let stream = loop {
        match connect(&endpoint).await {
            Ok(s) => break s,
            Err(_) if started.elapsed() < Duration::from_secs(20) => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(e) => {
                let error = tail.refine(CastError::with_detail(ErrorCode::PlayerUnresponsive, format!("IPC: {e}")));
                handle.player_event(PlayerEvent::LoadFailed { gen, error });
                return;
            }
        }
    };
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    for (id, name) in [(1, "duration"), (2, "time-pos")].into_iter().chain(INFO_PROPERTIES.iter().copied()) {
        let watch = format!(r#"{{"command":["observe_property",{id},"{name}"]}}"#);
        let _ = write.write_all(format!("{watch}\n").as_bytes()).await;
    }
    let mut info = InfoTracker::default();
    // mpv reports properties one by one, in bursts: report once they settle.
    let mut info_due: Option<tokio::time::Instant> = None;
    let (mut loaded, mut duration, mut last_progress) = (false, None, Instant::now());
    loop {
        tokio::select! {
            () = async { tokio::time::sleep_until(info_due.unwrap_or_else(tokio::time::Instant::now)).await }, if info_due.is_some() => {
                info_due = None;
                if let Some(report) = info.take_notable() {
                    handle.player_event(PlayerEvent::Info { gen, info: Box::new(report) });
                }
            }
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
                if info.observe(&msg) {
                    info_due = Some(tokio::time::Instant::now() + INFO_SETTLE);
                    continue;
                }
                match read_event(gen, &msg, &mut loaded, &mut duration, &mut last_progress) {
                    // mpv says only "loading failed"; its output usually says why.
                    Some(PlayerEvent::LoadFailed { gen, error }) => {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        handle.player_event(PlayerEvent::LoadFailed { gen, error: tail.refine(error) });
                    }
                    Some(event) => handle.player_event(event),
                    None => {}
                }
            }
        }
    }
}

/// mpv properties that describe the media, observed with ids from 10.
const INFO_PROPERTIES: &[(u32, &str)] = &[
    (10, "video-params"),
    (11, "video-format"),
    (12, "container-fps"),
    (13, "video-bitrate"),
    (14, "audio-codec-name"),
    (15, "audio-params"),
    (16, "audio-bitrate"),
    (17, "file-format"),
    (18, "hwdec-current"),
    (19, "hls-bitrate"),
];

/// How long mpv's property burst may last before the info is reported.
const INFO_SETTLE: Duration = Duration::from_millis(400);

/// Builds a [`MediaInfo`] from mpv's property changes and says when it's
/// worth reporting (see `MediaInfo::differs_notably`).
#[derive(Default)]
struct InfoTracker {
    now: MediaInfo,
    reported: MediaInfo,
}

impl InfoTracker {
    /// Takes in `msg` if it's a media property; true if it was one.
    fn observe(&mut self, msg: &serde_json::Value) -> bool {
        let name = match (msg["event"].as_str(), msg["name"].as_str()) {
            (Some("property-change"), Some(n)) if INFO_PROPERTIES.iter().any(|(_, p)| *p == n) => n,
            _ => return false,
        };
        let d = &msg["data"];
        let text = || d.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let bps = || d.as_f64().filter(|b| *b > 0.0).map(|b| b as u64);
        let video = self.now.video.get_or_insert_with(Default::default);
        match name {
            "video-params" => {
                video.width = d["w"].as_u64().map(|w| w as u32);
                video.height = d["h"].as_u64().map(|h| h as u32);
                video.pixel_format = d["pixelformat"].as_str().map(str::to_owned);
                video.hdr = match d["gamma"].as_str() {
                    Some("pq") => Some("HDR10".into()),
                    Some("hlg") => Some("HLG".into()),
                    _ => None,
                };
            }
            "video-format" => video.codec = text(),
            "container-fps" => video.fps = d.as_f64().filter(|f| *f > 0.0),
            "video-bitrate" => video.bitrate_bps = bps(),
            "file-format" => self.now.container = text().map(|f| container(&f)),
            "hwdec-current" => self.now.decoder = text().filter(|h| h != "no"),
            "hls-bitrate" => self.now.bitrate_bps = bps(),
            _ => {
                let audio = self.now.audio.get_or_insert_with(Default::default);
                match name {
                    "audio-codec-name" => audio.codec = text(),
                    "audio-params" => {
                        audio.sample_rate_hz = d["samplerate"].as_u64().map(|r| r as u32);
                        audio.channels = d["channel-count"].as_u64().map(|c| c as u16);
                    }
                    "audio-bitrate" => audio.bitrate_bps = bps(),
                    _ => {}
                }
            }
        }
        if self.now.video.as_ref().is_some_and(|v| *v == Default::default()) {
            self.now.video = None;
        }
        true
    }

    /// The info, if it changed notably since the last report.
    fn take_notable(&mut self) -> Option<MediaInfo> {
        if !self.now.differs_notably(&self.reported) {
            return None;
        }
        self.reported = self.now.clone();
        Some(self.now.clone())
    }
}

/// mpv's demuxer list ("hls,applehttp", "mov,mp4,m4a,3gp,3g2,mj2") to one name.
fn container(file_format: &str) -> String {
    let names: Vec<&str> = file_format.split(',').collect();
    let has = |n: &str| names.contains(&n);
    if has("hls") || has("applehttp") {
        "hls".into()
    } else if has("dash") {
        "dash".into()
    } else if has("mp4") || has("mov") {
        "mp4".into()
    } else if has("matroska") || has("webm") {
        "matroska".into()
    } else {
        names.first().copied().unwrap_or(file_format).to_owned()
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
            Some("error") => {
                let said = msg["file_error"].as_str().unwrap_or_default();
                let code = classify(said).unwrap_or(ErrorCode::Unknown);
                Some(PlayerEvent::LoadFailed { gen, error: CastError::with_detail(code, said) })
            }
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
    fn media_info_comes_from_properties() {
        let mut t = InfoTracker::default();
        let p = |name: &str, data: serde_json::Value| json!({"event":"property-change","name":name,"data":data});
        // A burst of properties is one report.
        for (name, data) in [
            ("video-params", json!({"w":1920,"h":1080,"pixelformat":"yuv420p","gamma":"bt.1886"})),
            ("video-format", json!("h264")),
            ("audio-params", json!({"samplerate":48000,"channel-count":2})),
            ("audio-codec-name", json!("aac")),
            ("file-format", json!("hls,applehttp")),
            ("hls-bitrate", json!(5_200_000.0)),
        ] {
            assert!(t.observe(&p(name, data)));
        }
        let i = t.take_notable().unwrap();
        assert_eq!((i.resolution_label(), i.container.as_deref()), (Some("1080p"), Some("hls")));
        assert_eq!(t.take_notable(), None, "nothing new");
        t.observe(&p("hls-bitrate", json!(5_300_000.0)));
        assert_eq!(t.take_notable(), None, "a wobble isn't reported");
        assert_eq!(t.now.summary(), "1080p · 1920×1080 · H.264 · 5.3 Mb/s | AAC · 48 kHz · stereo");
        assert!(!t.observe(&p("time-pos", json!(3.0))));
    }

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
