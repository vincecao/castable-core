//! Checks the session against the four rules of the TLA+ model
//! (`models/cast-session/CastReceiver.tla`):
//!
//! 1. the four bug paths the model found in a naive receiver, replayed step by step;
//! 2. an exhaustive search over every sequence of inputs up to a depth, with a
//!    fake player that also delivers late events from older loads.

use castable_core::*;
use std::collections::HashMap;
use std::time::Duration;

/// A player that does what it is told. It cancels a load on `Load`/`Stop`,
/// but events from older loads may still arrive late (that is a channel race
/// the session must survive).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct FakePlayer {
    loading: Option<Gen>,
    shown: Option<Gen>,
    playing: bool,
    issued: Vec<Gen>,
}

impl FakePlayer {
    fn apply(&mut self, cmd: &PlayerCommand) {
        match cmd {
            PlayerCommand::Load { gen, .. } => {
                self.loading = Some(*gen);
                self.shown = None;
                self.playing = false;
                self.issued.push(*gen);
            }
            PlayerCommand::Play => {
                assert!(self.shown.is_some(), "Play sent with nothing loaded");
                self.playing = true;
            }
            PlayerCommand::Pause => self.playing = false,
            PlayerCommand::Stop => {
                self.playing = false;
                self.loading = None;
            }
            PlayerCommand::Seek { .. } => assert!(self.shown.is_some(), "Seek sent with nothing loaded"),
        }
    }

    /// The player's own state change when it emits `ev` (none for a late event).
    fn emit(&mut self, ev: &PlayerEvent) {
        match ev {
            PlayerEvent::Loaded { gen, .. } if self.loading == Some(*gen) => {
                self.loading = None;
                self.shown = Some(*gen);
            }
            PlayerEvent::LoadFailed { gen, .. } if self.loading == Some(*gen) => self.loading = None,
            PlayerEvent::LoadFailed { gen, .. } if self.shown == Some(*gen) => {
                self.shown = None;
                self.playing = false;
            }
            PlayerEvent::Ended { gen } if self.shown == Some(*gen) && self.playing => self.playing = false,
            _ => {} // late or duplicate event: the player itself does nothing
        }
    }
}

/// The harness state: the session, the player, and whether the sender's last
/// intent was "play" (the model's `asked`).
#[derive(Debug, Clone)]
struct World {
    session: Session,
    player: FakePlayer,
    asked: bool,
    casts: usize,
}

impl World {
    fn new() -> Self {
        Self { session: Session::new(), player: FakePlayer::default(), asked: false, casts: 0 }
    }

    fn send(&mut self, input: Input) -> Result<(), UpnpError> {
        if let Input::Player(ev) = &input {
            self.player.emit(ev);
        }
        let before = self.session.phase().clone();
        match self.session.handle(input.clone()) {
            Ok(out) => {
                for c in &out.commands {
                    self.player.apply(c);
                }
            }
            Err(e) => {
                assert_eq!(self.session.phase(), &before, "an error must leave the session unchanged");
                return Err(e);
            }
        }
        self.track_intent(&input);
        Ok(())
    }

    fn track_intent(&mut self, input: &Input) {
        match input {
            Input::Sender(SenderCommand::SetUri(_)) => {
                self.asked = false;
                self.casts += 1;
            }
            Input::Sender(SenderCommand::Play) => self.asked = true,
            Input::Sender(SenderCommand::Pause | SenderCommand::Stop) => self.asked = false,
            // The video ended or failed: the old Play request is used up,
            // unless the session is still honouring it.
            Input::Player(PlayerEvent::Ended { .. } | PlayerEvent::LoadFailed { .. })
                if !matches!(self.session.phase(), Phase::Loading { then: AfterLoad::Play, .. })
                    && self.session.transport_state() != TransportState::Playing =>
            {
                self.asked = false
            }
            _ => {}
        }
    }

    fn current_gen(&self) -> Option<Gen> {
        self.session.cast().map(|c| c.gen)
    }

    /// The four rules of the model. Returns the name of the first broken rule.
    fn broken_rule(&self) -> Option<&'static str> {
        let state = self.session.transport_state();
        let p = &self.player;
        if p.playing != (state == TransportState::Playing) {
            return Some("ScreenMatchesState");
        }
        if p.shown.is_some() && p.shown != self.current_gen() {
            return Some("NeverShowsOldCast");
        }
        if self.asked && p.shown.is_some() && p.shown == self.current_gen() && p.loading.is_none() && !p.playing {
            return Some("PlayIsNeverLost");
        }
        if state == TransportState::Transitioning && p.loading != self.current_gen() {
            return Some("NeverStuckLoading");
        }
        None
    }

    fn inputs(&self) -> Vec<Input> {
        let mut v = vec![
            Input::Sender(SenderCommand::Play),
            Input::Sender(SenderCommand::Pause),
            Input::Sender(SenderCommand::Stop),
            Input::Sender(SenderCommand::Seek { to: Duration::from_secs(5) }),
        ];
        if self.casts < 2 {
            let url = if self.casts == 0 { "http://a/1.mp4" } else { "http://a/2.mp4" };
            v.push(Input::Sender(SenderCommand::SetUri(MediaDescriptor::video(url))));
        }
        for &gen in &self.player.issued {
            v.push(Input::Player(PlayerEvent::Loaded { gen, duration: Some(Duration::from_secs(60)) }));
            v.push(Input::Player(PlayerEvent::LoadFailed { gen, reason: "404".into() }));
            v.push(Input::Player(PlayerEvent::Ended { gen }));
        }
        v
    }

    fn key(&self) -> String {
        format!("{:?}|{:?}|{}|{}", self.session.phase(), self.player, self.asked, self.casts)
    }
}

fn cast(url: &str) -> Input {
    Input::Sender(SenderCommand::SetUri(MediaDescriptor::video(url)))
}
fn sender(c: SenderCommand) -> Input {
    Input::Sender(c)
}
fn loaded(gen: u64) -> Input {
    Input::Player(PlayerEvent::Loaded { gen: Gen(gen), duration: Some(Duration::from_secs(60)) })
}

fn replay(steps: Vec<Input>) -> World {
    let mut w = World::new();
    for (i, s) in steps.into_iter().enumerate() {
        let _ = w.send(s.clone());
        assert_eq!(w.broken_rule(), None, "step {i} ({s:?}) broke a rule: {w:#?}");
    }
    w
}

// ---- The four bug paths from the model, replayed against the real code ----

#[test]
fn bug_path_screen_matches_state() {
    // phone casts a video → it loads → you press Play → phone casts a new video
    let w = replay(vec![cast("http://a/1.mp4"), loaded(1), sender(SenderCommand::Play), cast("http://a/2.mp4")]);
    assert_eq!(w.session.transport_state(), TransportState::Transitioning);
    assert!(!w.player.playing, "the old video must stop when a new one is cast");
}

#[test]
fn bug_path_never_shows_old_cast() {
    // phone casts a video → phone casts a new video → the old video finishes loading
    let w = replay(vec![cast("http://a/1.mp4"), cast("http://a/2.mp4"), loaded(1)]);
    assert_eq!(w.player.shown, None);
    assert_eq!(w.session.transport_state(), TransportState::Transitioning);
}

#[test]
fn bug_path_play_is_never_lost() {
    // phone casts a video → you press Play while it loads → it loads
    let w = replay(vec![cast("http://a/1.mp4"), sender(SenderCommand::Play), loaded(1)]);
    assert_eq!(w.session.transport_state(), TransportState::Playing);
    assert!(w.player.playing);
}

#[test]
fn bug_path_never_stuck_loading() {
    // phone casts a video → the video fails to load
    let w = replay(vec![
        cast("http://a/1.mp4"),
        Input::Player(PlayerEvent::LoadFailed { gen: Gen(1), reason: "404".into() }),
    ]);
    assert_eq!(w.session.transport_state(), TransportState::Stopped);
    assert_eq!(w.session.transport_status(), "ERROR_OCCURRED");
}

#[test]
fn play_after_a_failed_load_retries() {
    let mut w = replay(vec![
        cast("http://a/1.mp4"),
        Input::Player(PlayerEvent::LoadFailed { gen: Gen(1), reason: "timeout".into() }),
        sender(SenderCommand::Play),
    ]);
    assert_eq!(w.session.transport_state(), TransportState::Transitioning);
    w.send(loaded(2)).unwrap();
    assert!(w.player.playing);
}

#[test]
fn errors_match_upnp() {
    let mut w = World::new();
    assert_eq!(w.send(sender(SenderCommand::Play)), Err(UpnpError::TransitionNotAvailable));
    w.send(cast("http://a/1.mp4")).unwrap();
    w.send(loaded(1)).unwrap();
    assert_eq!(w.send(sender(SenderCommand::Seek { to: Duration::from_secs(600) })), Err(UpnpError::IllegalSeekTarget));
}

#[test]
fn a_seek_while_loading_starts_there() {
    // A phone casts, seeks to 0:30 and presses Play before the video has loaded.
    let mut w = replay(vec![cast("http://a/1.mp4"), sender(SenderCommand::Seek { to: Duration::from_secs(30) })]);
    w.send(sender(SenderCommand::Play)).unwrap();
    let before = w.player.clone();
    w.send(loaded(1)).unwrap();
    assert!(w.player.playing);
    assert_ne!(before, w.player);
    assert_eq!(w.session.cast().unwrap().position, Duration::from_secs(30));
}

#[test]
fn a_seek_while_loading_a_live_stream_is_refused() {
    let mut live = MediaDescriptor::video("http://a/live.m3u8");
    live.delivery = Delivery::Live;
    let mut w = World::new();
    w.send(Input::Sender(SenderCommand::SetUri(live))).unwrap();
    assert_eq!(
        w.send(sender(SenderCommand::Seek { to: Duration::from_secs(5) })),
        Err(UpnpError::SeekModeNotSupported)
    );
}

// ---- Exhaustive check, like TLC but over the real code ----

#[test]
fn every_input_sequence_keeps_the_four_rules() {
    const DEPTH: usize = 14;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut stack = vec![(World::new(), DEPTH, Vec::<Input>::new())];
    let mut explored = 0usize;
    while let Some((w, left, path)) = stack.pop() {
        explored += 1;
        if let Some(rule) = w.broken_rule() {
            panic!("{rule} breaks after {} steps:\n{path:#?}", path.len());
        }
        if left == 0 {
            continue;
        }
        let key = w.key();
        if seen.get(&key).is_some_and(|&l| l >= left) {
            continue;
        }
        seen.insert(key, left);
        for input in w.inputs() {
            let mut next = w.clone();
            if next.send(input.clone()).is_ok() {
                let mut p = path.clone();
                p.push(input);
                stack.push((next, left - 1, p));
            }
        }
    }
    eprintln!("explored {explored} steps, {} distinct states", seen.len());
    assert!(seen.len() > 20, "the search should reach many states");
}
