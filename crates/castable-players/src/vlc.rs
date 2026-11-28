//! The "vlc-http" protocol: VLC's web remote, on 127.0.0.1 only, with a
//! random password per cast.

use crate::diagnose::Tail;
use castable_net::{AudioInfo, CastError, ErrorCode, Gen, Handle, MediaInfo, PlayerCommand, PlayerEvent, VideoInfo};
use std::time::{Duration, Instant};
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
        PlayerCommand::SetRate { rate } => vec![format!("rate&val={rate}")],
        PlayerCommand::SelectTrack { .. } => vec![], // the web remote can't pick tracks
        PlayerCommand::Load { .. } => vec![],        // a load starts a new player instead
    }
}

/// Where one VLC's web remote listens.
pub struct Remote {
    pub port: u16,
    pub password: String,
    http: reqwest::Client,
}

impl Remote {
    pub fn new() -> Self {
        // A free port, released for VLC to take a moment later.
        let port =
            std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).map_or(8080, |a| a.port());
        Self { port, password: uuid::Uuid::new_v4().simple().to_string(), http: reqwest::Client::new() }
    }

    /// GET /requests/status.json[?command=...]; returns the JSON body.
    async fn status(&self, command: Option<&str>) -> reqwest::Result<serde_json::Value> {
        let base = format!("http://127.0.0.1:{}/requests/status.json", self.port);
        let url = command.map_or(base.clone(), |c| format!("{base}?command={c}"));
        self.http
            .get(url)
            .basic_auth("", Some(&self.password))
            .timeout(Duration::from_secs(2))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
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
    let mut reported = MediaInfo::default();
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
                if let Some(info) = media_info(&status).filter(|i| i.differs_notably(&reported)) {
                    reported = info.clone();
                    handle.player_event(PlayerEvent::Info { gen, info: Box::new(info) });
                }
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

/// The media info VLC's status lists under `information.category`, one entry
/// per stream ("Stream 0": Type, Codec, Video_resolution, Frame_rate,
/// Sample_rate, Channels). Bitrates aren't reliable there, so none.
fn media_info(status: &serde_json::Value) -> Option<MediaInfo> {
    let streams = status["information"]["category"].as_object()?;
    let mut info = MediaInfo::default();
    for stream in streams.values() {
        let field = |k: &str| stream[k].as_str().map(str::trim).filter(|v| !v.is_empty());
        let codec = field("Codec").map(vlc_codec);
        match field("Type") {
            Some("Video") => {
                let size = field("Video_resolution").or(field("Buffer_dimensions"));
                let (w, h) = size.and_then(|s| s.split_once('x')).unzip();
                info.video = Some(VideoInfo {
                    codec,
                    width: w.and_then(|w| w.trim().parse().ok()),
                    height: h.and_then(|h| h.trim().parse().ok()),
                    fps: field("Frame_rate").and_then(|f| f.parse().ok()),
                    ..Default::default()
                });
            }
            Some("Audio") => {
                info.audio = Some(AudioInfo {
                    codec,
                    sample_rate_hz: field("Sample_rate").and_then(|r| r.trim_end_matches(" Hz").parse().ok()),
                    channels: field("Channels").and_then(|c| match c {
                        "Mono" => Some(1),
                        "Stereo" => Some(2),
                        c if c.contains("5.1") => Some(6),
                        c if c.contains("7.1") => Some(8),
                        c => c.parse().ok(),
                    }),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
    (!info.is_empty()).then_some(info)
}

/// VLC's "H264 - MPEG-4 AVC (part 10) (avc1)" → "h264", by its FourCC.
fn vlc_codec(codec: &str) -> String {
    let fourcc = codec.rsplit_once('(').map_or(codec, |(_, f)| f.trim_end_matches(')')).trim().to_ascii_lowercase();
    match fourcc.as_str() {
        "avc1" | "h264" => "h264",
        "hevc" | "hvc1" | "hev1" => "hevc",
        "av01" => "av1",
        "vp09" | "vp90" => "vp9",
        "mp4a" => "aac",
        "a52 " | "a52" => "ac3",
        "eac3" => "eac3",
        "mpga" | "mp3 " => "mp3",
        "opus" => "opus",
        "flac" => "flac",
        other => return other.to_owned(),
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_info_from_vlc_status() {
        let status = serde_json::json!({"information":{"category":{
            "meta":{"filename":"x.m3u8"},
            "Stream 0":{"Type":"Video","Codec":"H264 - MPEG-4 AVC (part 10) (avc1)","Video_resolution":"1920x1080","Frame_rate":"23.976024"},
            "Stream 1":{"Type":"Audio","Codec":"MPEG AAC Audio (mp4a)","Sample_rate":"48000 Hz","Channels":"Stereo"}
        }}});
        let i = media_info(&status).unwrap();
        assert_eq!(i.summary(), "1080p · 1920×1080 · H.264 · 23.98 fps | AAC · 48 kHz · stereo");
        assert_eq!(media_info(&serde_json::json!({"state":"playing"})), None);
    }

    #[test]
    fn controls_map_to_remote_commands() {
        assert_eq!(encode(&PlayerCommand::SetVolume { volume: 100 }), ["volume&val=256"]);
        assert_eq!(encode(&PlayerCommand::Seek { to: Duration::from_secs(90) }), ["seek&val=90"]);
    }
}
