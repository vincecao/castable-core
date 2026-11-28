//! Golden fixtures shared with TypeScript. `packages/protocol` type-checks
//! `fixtures/views.ts` against its `ReceiverView` union, so a change in the
//! Rust shape that TypeScript doesn't follow fails CI.
//!
//! Regenerate with `UPDATE_FIXTURES=1 cargo test --test fixtures`.

use castable_core::*;
use std::path::PathBuf;
use std::time::Duration;

/// A screen view: the session's, with receiver-wide state (volume, sleep timer).
fn screen(s: &Session, volume: u8, muted: bool, sleep_ends_at_ms: Option<u64>) -> serde_json::Value {
    serde_json::to_value(ReceiverView { session: s.view(), volume, muted, sleep_ends_at_ms }).unwrap()
}

fn views() -> String {
    let mut s = Session::new();
    let mut out = vec![screen(&s, 50, false, None)];
    let mut media = MediaDescriptor::video("http://192.0.2.10/e1.mp4");
    media.title = Some("Episode 1".into());
    media.mime = Some("video/mp4".into());
    let step = |s: &mut Session, out: &mut Vec<serde_json::Value>, input: Input| {
        s.handle(input).unwrap();
        out.push(screen(s, 50, false, None));
    };
    step(&mut s, &mut out, Input::Sender(SenderCommand::SetUri(media)));
    step(&mut s, &mut out, Input::Sender(SenderCommand::Play));
    step(
        &mut s,
        &mut out,
        Input::Player(PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(1500)) }),
    );
    // What the player found out; not a view of its own (keeps the view numbers).
    s.handle(Input::Player(PlayerEvent::Info { gen: Gen(1), info: Box::new(sample_info()) })).unwrap();
    s.handle(Input::Player(PlayerEvent::Tracks { gen: Gen(1), tracks: sample_tracks() })).unwrap();
    step(
        &mut s,
        &mut out,
        Input::Player(PlayerEvent::Progress {
            gen: Gen(1),
            position: Duration::from_secs(724),
            duration: Some(Duration::from_secs(1500)),
        }),
    );
    step(&mut s, &mut out, Input::Screen(ScreenCommand { gen: Gen(1), command: ScreenAction::SetRate { rate: 1.5 } }));
    step(&mut s, &mut out, Input::Sender(SenderCommand::Pause));
    // Paused at night: the volume turned down and a sleep timer on (2026-01-01 00:30 UTC).
    s.handle(Input::Screen(ScreenCommand {
        gen: Gen(1),
        command: ScreenAction::SelectTrack { kind: TrackKind::Audio, id: Some("2".into()) },
    }))
    .unwrap();
    out.push(screen(&s, 20, false, Some(1_767_227_400_000)));
    step(&mut s, &mut out, Input::Sender(SenderCommand::Stop));
    step(
        &mut s,
        &mut out,
        Input::Player(PlayerEvent::LoadFailed {
            gen: Gen(1),
            error: CastError::with_detail(ErrorCode::Network, "network error"),
        }),
    );
    // A live stream, playing.
    let mut live = MediaDescriptor::video("http://192.0.2.10/news.m3u8");
    live.title = Some("Evening news".into());
    live.delivery = Delivery::Live;
    step(&mut s, &mut out, Input::Sender(SenderCommand::SetUri(live)));
    step(&mut s, &mut out, Input::Sender(SenderCommand::Play));
    step(&mut s, &mut out, Input::Player(PlayerEvent::Loaded { gen: Gen(2), duration: None }));
    serde_json::to_string_pretty(&out).unwrap()
}

fn sample_info() -> MediaInfo {
    MediaInfo {
        container: Some("hls".into()),
        bitrate_bps: Some(5_200_000),
        decoder: Some("videotoolbox".into()),
        video: Some(VideoInfo {
            codec: Some("h264".into()),
            width: Some(1920),
            height: Some(1080),
            fps: Some(23.976),
            ..Default::default()
        }),
        audio: Some(AudioInfo {
            codec: Some("aac".into()),
            sample_rate_hz: Some(48_000),
            channels: Some(2),
            bitrate_bps: None,
        }),
    }
}

fn sample_tracks() -> Vec<MediaTrack> {
    let track = |id: &str, kind, selected| MediaTrack {
        id: id.into(),
        kind,
        label: None,
        lang: None,
        codec: None,
        width: None,
        height: None,
        bitrate_bps: None,
        channels: None,
        selected,
    };
    vec![
        MediaTrack {
            codec: Some("h264".into()),
            width: Some(1920),
            height: Some(1080),
            ..track("1", TrackKind::Video, true)
        },
        MediaTrack {
            codec: Some("h264".into()),
            width: Some(1280),
            height: Some(720),
            ..track("2", TrackKind::Video, false)
        },
        MediaTrack {
            label: Some("English".into()),
            lang: Some("en".into()),
            codec: Some("aac".into()),
            channels: Some(2),
            ..track("1", TrackKind::Audio, true)
        },
        MediaTrack {
            label: Some("English (commentary)".into()),
            lang: Some("en".into()),
            codec: Some("ac3".into()),
            channels: Some(6),
            ..track("2", TrackKind::Audio, false)
        },
        MediaTrack { lang: Some("en".into()), codec: Some("subrip".into()), ..track("1", TrackKind::Subtitle, false) },
    ]
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn views_match_the_fixture() {
    let json = views();
    let ts = format!(
        "// Generated by crates/castable-core/tests/fixtures.rs. Do not edit.\nimport type {{ ReceiverView }} from \"../packages/protocol/src/index.ts\";\n\nexport const views = {json} as const satisfies readonly ReceiverView[];\n"
    );
    check(&[(root().join("fixtures/views.json"), json + "\n"), (root().join("fixtures/views.ts"), ts)]);
}

fn protocol() -> String {
    let media = MediaDescriptor::video("http://192.0.2.10/e1.mp4");
    // A start position, so its field name is checked against the TypeScript types.
    let mut resume = MediaDescriptor::video("http://192.0.2.10/e2.mp4");
    resume.delivery = Delivery::OnDemand { start_at_secs: Some(90.5) };
    let commands = vec![
        PlayerCommand::Load { gen: Gen(1), media: media.clone() },
        PlayerCommand::Load { gen: Gen(2), media: resume },
        PlayerCommand::Play,
        PlayerCommand::Pause,
        PlayerCommand::Stop,
        PlayerCommand::Seek { to: Duration::from_millis(30_500) },
        PlayerCommand::SetVolume { volume: 70 },
        PlayerCommand::SetMuted { muted: true },
        PlayerCommand::SetRate { rate: 1.5 },
        PlayerCommand::SelectTrack { kind: TrackKind::Audio, id: Some("2".into()) },
        PlayerCommand::SelectTrack { kind: TrackKind::Subtitle, id: None },
    ];
    let events = vec![
        PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(90)) },
        PlayerEvent::Loaded { gen: Gen(2), duration: None },
        PlayerEvent::LoadFailed {
            gen: Gen(1),
            error: CastError::with_detail(ErrorCode::NotFound, "HTTP error 404 Not Found"),
        },
        PlayerEvent::LoadFailed { gen: Gen(2), error: CastError::new(ErrorCode::PlayerCrashed) },
        PlayerEvent::Progress {
            gen: Gen(1),
            position: Duration::from_secs(12),
            duration: Some(Duration::from_secs(90)),
        },
        PlayerEvent::Ended { gen: Gen(1) },
        PlayerEvent::Closed { gen: Gen(1) },
        PlayerEvent::Info { gen: Gen(1), info: Box::new(sample_info()) },
        PlayerEvent::Tracks { gen: Gen(1), tracks: sample_tracks() },
    ];
    let sender = vec![
        SenderCommand::SetUri(media),
        SenderCommand::Play,
        SenderCommand::Pause,
        SenderCommand::Stop,
        SenderCommand::Seek { to: Duration::from_secs(10) },
    ];
    let screen = vec![
        ScreenCommand { gen: Gen(1), command: ScreenAction::Pause },
        ScreenCommand { gen: Gen(2), command: ScreenAction::Seek { to: Duration::from_secs(40) } },
        ScreenCommand { gen: Gen(1), command: ScreenAction::Close },
        ScreenCommand { gen: Gen(1), command: ScreenAction::SetVolume { volume: 35 } },
        ScreenCommand { gen: Gen(1), command: ScreenAction::SetMuted { muted: true } },
        ScreenCommand { gen: Gen(1), command: ScreenAction::SetRate { rate: 1.25 } },
        ScreenCommand {
            gen: Gen(1),
            command: ScreenAction::SelectTrack { kind: TrackKind::Video, id: Some("2".into()) },
        },
        ScreenCommand { gen: Gen(1), command: ScreenAction::SetSleepTimer { minutes: Some(30) } },
        ScreenCommand { gen: Gen(1), command: ScreenAction::SetSleepTimer { minutes: None } },
    ];
    // Every value must also read back as itself.
    for c in &screen {
        let back: ScreenCommand = serde_json::from_value(serde_json::to_value(c).unwrap()).unwrap();
        assert_eq!(&back, c);
    }
    for e in &events {
        let back: PlayerEvent = serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap();
        assert_eq!(&back, e);
    }
    for c in &sender {
        let back: SenderCommand = serde_json::from_value(serde_json::to_value(c).unwrap()).unwrap();
        assert_eq!(&back, c);
    }
    serde_json::to_string_pretty(
        &serde_json::json!({ "commands": commands, "events": events, "sender": sender, "screen": screen }),
    )
    .unwrap()
}

#[test]
fn protocol_matches_the_fixture() {
    let json = protocol();
    let ts = format!(
        "// Generated by crates/castable-core/tests/fixtures.rs. Do not edit.\nimport type {{ PlayerCommand, PlayerEvent, ScreenCommand, SenderCommand }} from \"../packages/protocol/src/index.ts\";\n\nexport const protocol = {json} as const satisfies {{\n  commands: readonly PlayerCommand[];\n  events: readonly PlayerEvent[];\n  sender: readonly SenderCommand[];\n  screen: readonly ScreenCommand[];\n}};\n"
    );
    check(&[(root().join("fixtures/protocol.json"), json + "\n"), (root().join("fixtures/protocol.ts"), ts)]);
}

/// Every error code, so TypeScript's `ErrorCode` can't fall behind.
#[test]
fn errors_match_the_fixture() {
    let all: Vec<CastError> = ErrorCode::ALL.iter().map(|&c| CastError::new(c)).collect();
    let json = serde_json::to_string_pretty(&all).unwrap();
    let ts = format!(
        "// Generated by crates/castable-core/tests/fixtures.rs. Do not edit.\nimport type {{ CastError }} from \"../packages/protocol/src/index.ts\";\n\nexport const errors = {json} as const satisfies readonly CastError[];\n"
    );
    check(&[(root().join("fixtures/errors.json"), json + "\n"), (root().join("fixtures/errors.ts"), ts)]);
}

/// docs/ERRORS.md is generated from the same table, so it can't drift.
#[test]
fn errors_doc_matches_the_catalog() {
    let mut md = String::from(
        "# Error codes\n\n<!-- Generated by crates/castable-core/tests/fixtures.rs from ErrorCode. Do not edit; run UPDATE_FIXTURES=1 cargo test. -->\n\n\
         Every error Castable reports has a code. The first digit is its category, the same for every player and platform. \
         Logs show `code` (`E403`), `error` (`forbidden`), `category` and, when the source said something, `detail`. \
         To add a code, add one line to the `codes!` table in `crates/castable-core/src/errors.rs`; screens show the category's message until they get a specific one.\n",
    );
    let mut category = None;
    for &c in ErrorCode::ALL {
        if category != Some(c.category()) {
            category = Some(c.category());
            md.push_str(&format!(
                "\n## {} ({}xx)\n\n| Code | Name | Meaning |\n| --- | --- | --- |\n",
                c.category().name(),
                c.category().digit()
            ));
        }
        md.push_str(&format!("| {} | `{}` | {} |\n", c.id(), c.name(), c.summary()));
    }
    check(&[(root().join("docs/ERRORS.md"), md)]);
}

fn check(files: &[(PathBuf, String)]) {
    for (path, content) in files {
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::write(path, content).unwrap();
        }
        // A Windows checkout may turn \n into \r\n; the content is what matters.
        let on_disk = std::fs::read_to_string(path).unwrap_or_default().replace("\r\n", "\n");
        assert_eq!(&on_disk, content, "{} is out of date; run UPDATE_FIXTURES=1 cargo test", path.display());
    }
}

#[test]
fn descriptor_round_trips_through_json() {
    let json = r#"{"url":"https://media.example.com/live.m3u8","kind":"video","mime":"application/vnd.apple.mpegurl","headers":{"Referer":"https://media.example.com/"},"delivery":{"type":"live"}}"#;
    let m: MediaDescriptor = serde_json::from_str(json).unwrap();
    assert!(m.is_live());
    assert_eq!(serde_json::to_string(&m).unwrap(), json);
    // A start position goes over the wire in camelCase, as `packages/protocol` declares it.
    let resume = r#"{"url":"https://media.example.com/e1.mp4","kind":"video","delivery":{"type":"onDemand","startAtSecs":90.5}}"#;
    let m: MediaDescriptor = serde_json::from_str(resume).unwrap();
    assert_eq!(m.delivery, Delivery::OnDemand { start_at_secs: Some(90.5) });
    assert_eq!(serde_json::to_string(&m).unwrap(), resume);
}
