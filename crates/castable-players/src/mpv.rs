//! The "mpv-ipc" protocol: mpv's JSON IPC socket (mpv, IINA, other mpv front-ends).
//!
//! An external player starts one mpv per cast. An app that embeds libmpv keeps
//! one mpv running, opens its IPC socket (`input-ipc-server`), and for each
//! cast runs [`session_from`] with [`load`] as the first messages: the same
//! events, errors and media info as an external mpv.

use crate::diagnose::{classify, Tail};
use castable_net::{
    CastError, ErrorCode, Gen, Handle, MediaDescriptor, MediaInfo, MediaTrack, PlayerCommand, PlayerEvent, TrackKind,
};
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
        PlayerCommand::SetRate { rate } => vec![set("speed", &rate.to_string())],
        PlayerCommand::SelectTrack { kind, id } => {
            let prop = match kind {
                TrackKind::Audio => "aid",
                TrackKind::Video => "vid",
                TrackKind::Subtitle => "sid",
            };
            // None: subtitles off; video back to mpv's own choice.
            let value = match (id, kind) {
                (Some(id), _) => serde_json::Value::String(id.clone()),
                (None, TrackKind::Subtitle) => "no".into(),
                (None, _) => "auto".into(),
            };
            vec![serde_json::json!({ "command": ["set_property", prop, value] }).to_string()]
        }
        PlayerCommand::Load { .. } => vec![], // a load starts a new player instead
    }
}

/// The IPC messages that load `media` into a running mpv, paused (the core
/// says when to play): its request headers and title, then the file. Every
/// option is set on each load, so nothing carries over from the last cast.
pub fn load(media: &MediaDescriptor) -> Vec<String> {
    let set =
        |prop: &str, v: serde_json::Value| serde_json::json!({ "command": ["set_property", prop, v] }).to_string();
    let header = |name: &str| media.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let fields: Vec<String> = media
        .headers
        .iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("user-agent") && !k.eq_ignore_ascii_case("referer"))
        .filter(|(k, v)| !(k.contains(['\r', '\n']) || v.contains(['\r', '\n'])))
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    vec![
        set("pause", true.into()),
        set("speed", 1.into()),
        // Track choices are options in mpv and would carry over to the next file.
        set("aid", "auto".into()),
        set("vid", "auto".into()),
        set("sid", "auto".into()),
        set("user-agent", header("user-agent").unwrap_or("libmpv").into()),
        set("referrer", header("referer").unwrap_or("").into()),
        set("http-header-fields", fields.into()),
        set("force-media-title", media.title.clone().unwrap_or_default().into()),
        serde_json::json!({ "command": ["loadfile", media.url, "replace"] }).to_string(),
    ]
}

/// Where a player's IPC endpoint lives: a socket in this run's private
/// folder ([`private_dir`]), or a named pipe on Windows. Remove the socket
/// when the player closes (a killed mpv can't).
pub fn endpoint(session_id: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\castable-{session_id}-{}", uuid::Uuid::new_v4().simple()))
    } else {
        private_dir().join(format!("{session_id}.sock"))
    }
}

/// A folder only this user can open (0700), made once per run under
/// `$XDG_RUNTIME_DIR` or the temp folder, with a random name: another local
/// user can't guess it, create it first, or put a socket of theirs in it.
#[cfg(unix)]
pub fn private_dir() -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        for _ in 0..8 {
            // Short, so socket paths stay under the 104-byte limit.
            let id = uuid::Uuid::new_v4().simple().to_string();
            let dir = base.join(format!("castable-{}", &id[..12]));
            if std::fs::DirBuilder::new().mode(0o700).create(&dir).is_ok() {
                return dir;
            }
        }
        base
    })
    .clone()
}

#[cfg(not(unix))]
pub fn private_dir() -> PathBuf {
    std::env::temp_dir()
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
    controls: UnboundedReceiver<PlayerCommand>,
    handle: Handle,
    tail: Tail,
) {
    session_from(gen, endpoint, controls, handle, tail, Vec::new()).await
}

/// [`session`], sending `first` once the properties are observed: for an
/// embedded mpv, the [`load`] of this cast, so no event of it is missed.
pub async fn session_from(
    gen: Gen,
    endpoint: PathBuf,
    mut controls: UnboundedReceiver<PlayerCommand>,
    handle: Handle,
    tail: Tail,
    first: Vec<String>,
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
    for (id, name) in
        [(1, "duration"), (2, "time-pos"), (3, "track-list")].into_iter().chain(INFO_PROPERTIES.iter().copied())
    {
        let watch = format!(r#"{{"command":["observe_property",{id},"{name}"]}}"#);
        let _ = write.write_all(format!("{watch}\n").as_bytes()).await;
    }
    for msg in first {
        let _ = write.write_all(format!("{msg}\n").as_bytes()).await;
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
            // "no": FFmpeg decodes in software.
            "hwdec-current" => self.now.decoder = text().map(|h| if h == "no" { "software".into() } else { h }),
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

/// mpv's `track-list` as the core's tracks (its ids are what `aid`/`vid`/`sid` take).
fn tracks(list: &[serde_json::Value]) -> Vec<MediaTrack> {
    list.iter()
        .filter_map(|t| {
            let kind = match t["type"].as_str()? {
                "audio" => TrackKind::Audio,
                "video" if !t["albumart"].as_bool().unwrap_or(false) => TrackKind::Video,
                "sub" => TrackKind::Subtitle,
                _ => return None,
            };
            let text = |k: &str| t[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
            let num = |k: &str| t[k].as_u64().filter(|n| *n > 0);
            Some(MediaTrack {
                id: t["id"].as_i64()?.to_string(),
                kind,
                label: text("title"),
                lang: text("lang"),
                codec: text("codec"),
                width: num("demux-w").map(|n| n as u32),
                height: num("demux-h").map(|n| n as u32),
                bitrate_bps: num("demux-bitrate").or_else(|| num("hls-bitrate")),
                channels: num("demux-channel-count").map(|n| n as u32),
                selected: t["selected"].as_bool().unwrap_or(false),
            })
        })
        .collect()
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
        (Some("property-change"), Some("track-list")) => {
            Some(PlayerEvent::Tracks { gen, tracks: tracks(msg["data"].as_array()?) })
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
        assert_eq!((i.resolution_label().as_deref(), i.container.as_deref()), (Some("1080p"), Some("hls")));
        assert_eq!(t.take_notable(), None, "nothing new");
        t.observe(&p("hls-bitrate", json!(5_300_000.0)));
        assert_eq!(t.take_notable(), None, "a wobble isn't reported");
        assert_eq!(t.now.summary(), "1080p · 1920×1080 · H.264 · 5.3 Mb/s | AAC · 48 kHz · stereo");
        assert!(!t.observe(&p("time-pos", json!(3.0))));
    }

    #[test]
    fn tracks_come_from_the_track_list() {
        let (mut loaded, mut duration, mut last) = (true, None, Instant::now());
        let list = json!([
            {"id":1,"type":"video","codec":"hevc","demux-w":1920,"demux-h":1080,"selected":true},
            {"id":1,"type":"audio","lang":"eng","title":"English","codec":"aac","demux-channel-count":6,"selected":true},
            {"id":2,"type":"audio","lang":"fre","codec":"ac3","demux-channel-count":2,"selected":false},
            {"id":1,"type":"sub","lang":"eng","codec":"subrip","selected":false},
            {"id":2,"type":"video","albumart":true,"codec":"mjpeg"}
        ]);
        let msg = json!({"event":"property-change","name":"track-list","data":list});
        let Some(PlayerEvent::Tracks { tracks, .. }) = read_event(Gen(1), &msg, &mut loaded, &mut duration, &mut last)
        else {
            panic!("no tracks event")
        };
        let summary: Vec<_> = tracks.iter().map(|t| (t.kind, t.id.as_str(), t.selected)).collect();
        assert_eq!(
            summary,
            [
                (TrackKind::Video, "1", true),
                (TrackKind::Audio, "1", true),
                (TrackKind::Audio, "2", false),
                (TrackKind::Subtitle, "1", false)
            ]
        );
        assert_eq!(
            (tracks[0].height, tracks[1].channels, tracks[1].label.as_deref()),
            (Some(1080), Some(6), Some("English"))
        );
    }

    #[test]
    fn speed_and_tracks_are_properties() {
        assert_eq!(encode(&PlayerCommand::SetRate { rate: 1.5 }), [r#"{"command":["set_property","speed",1.5]}"#]);
        let audio = PlayerCommand::SelectTrack { kind: TrackKind::Audio, id: Some("2".into()) };
        assert_eq!(encode(&audio), [r#"{"command":["set_property","aid","2"]}"#]);
        let subs_off = PlayerCommand::SelectTrack { kind: TrackKind::Subtitle, id: None };
        assert_eq!(encode(&subs_off), [r#"{"command":["set_property","sid","no"]}"#]);
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

    #[cfg(unix)]
    #[test]
    fn sockets_go_in_a_private_folder() {
        use std::os::unix::fs::PermissionsExt;
        let socket = endpoint("1-1");
        let dir = socket.parent().unwrap();
        assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(endpoint("1-2").parent(), Some(dir), "one folder per run");
        assert!(socket.as_os_str().len() < 104, "{}", socket.display());
    }

    #[test]
    fn a_load_sets_every_option_then_opens_the_file_paused() {
        let mut media = MediaDescriptor::from_url_and_mime("https://media.example.com/e1.m3u8", None);
        media.title = Some("Episode 1".into());
        media.headers.insert("Referer".into(), "https://media.example.com/".into());
        media.headers.insert("X-Token".into(), "abc".into());
        let msgs: Vec<serde_json::Value> = load(&media).iter().map(|m| serde_json::from_str(m).unwrap()).collect();
        assert_eq!(msgs[0], json!({"command":["set_property","pause",true]}));
        assert_eq!(msgs[1], json!({"command":["set_property","speed",1]}));
        // Track choices are reset, so the last cast's subtitles-off or audio track 2 doesn't carry over.
        assert_eq!(
            msgs[2..5],
            [
                json!({"command":["set_property","aid","auto"]}),
                json!({"command":["set_property","vid","auto"]}),
                json!({"command":["set_property","sid","auto"]})
            ]
        );
        assert_eq!(msgs[5], json!({"command":["set_property","user-agent","libmpv"]}));
        assert_eq!(msgs[6], json!({"command":["set_property","referrer","https://media.example.com/"]}));
        assert_eq!(msgs[7], json!({"command":["set_property","http-header-fields",["X-Token: abc"]]}));
        assert_eq!(msgs[8], json!({"command":["set_property","force-media-title","Episode 1"]}));
        assert_eq!(msgs[9], json!({"command":["loadfile","https://media.example.com/e1.m3u8","replace"]}));
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
