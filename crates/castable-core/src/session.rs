//! The cast session: one state value, one transition function.
//!
//! This is the planned design checked in `models/cast-session/CastReceiver.tla`.
//! The four rules from that model hold *by construction* here:
//!
//! - **What senders see matches the screen**: the transport state is computed
//!   from [`Phase`]; it is never stored separately.
//! - **A replaced cast never stays on screen**: there is only ever one
//!   [`Cast`]; a new cast replaces it and bumps the [`Gen`], and player events
//!   from an older generation are ignored.
//! - **Pressing Play is never lost**: Play during loading becomes
//!   [`AfterLoad::Play`] instead of error 701.
//! - **"Loading" only while something loads**: a failed or stopped load moves
//!   to [`Phase::Unloaded`], which reports Stopped.

use crate::descriptor::MediaDescriptor;
use crate::error::UpnpError;
use crate::errors::CastError;
use crate::media_info::MediaInfo;
use crate::tracks::{self, MediaTrack, TrackKind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Generation of a load. Each new load gets a new one, so late results from
/// an older load can be recognised and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Gen(pub u64);

/// The media currently cast to the receiver.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub gen: Gen,
    pub media: MediaDescriptor,
    pub position: Duration,
    pub duration: Option<Duration>,
    /// What the player reported about the media (resolution, codecs, ...).
    pub info: MediaInfo,
    /// Playback speed (1.0 is normal).
    pub rate: f64,
    /// The tracks the player found, with the selected ones marked.
    pub tracks: Vec<MediaTrack>,
}

/// The speeds a screen may set.
pub const RATES: std::ops::RangeInclusive<f64> = 0.25..=4.0;

/// What to do once loading finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterLoad {
    Hold,
    Play,
}

/// Playback of loaded media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Playback {
    Stopped,
    Playing,
    Paused,
}

/// Why media is not loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unloaded {
    StoppedWhileLoading,
    LoadFailed(CastError),
    /// The player went away (its process exited, its window was closed).
    PlayerClosed,
}

/// The whole session state. Each variant carries exactly the data that is
/// valid in it, so impossible combinations cannot be written down.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Waiting for a cast.
    Idle,
    Loading {
        cast: Cast,
        then: AfterLoad,
    },
    Loaded {
        cast: Cast,
        playback: Playback,
    },
    /// A cast is set but nothing is loaded (stopped mid-load or load failed).
    Unloaded {
        cast: Cast,
        why: Unloaded,
    },
}

/// What the UI renders. Mirrors `ReceiverView` in `packages/protocol`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View<'a> {
    pub transport_state: &'static str,
    /// The cast this view shows. The screen sends it back with each press,
    /// so a press made on a replaced cast is dropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen: Option<Gen>,
    /// What to call the cast on screen: the sender's title, else the decoded
    /// file name (`MediaDescriptor::display_title`), so no UI decodes URLs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(flatten)]
    pub phase: PhaseView<'a>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PhaseView<'a> {
    Idle,
    Loading {
        media: &'a MediaDescriptor,
        play_when_ready: bool,
    },
    Loaded {
        media: &'a MediaDescriptor,
        playback: Playback,
        position_secs: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_secs: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        info: Option<&'a MediaInfo>,
        /// A short quality badge from `info`: "1080p · HDR10".
        #[serde(skip_serializing_if = "Option::is_none")]
        quality_label: Option<String>,
        /// Playback speed (1 is normal).
        rate: f64,
        /// The tracks the player found; empty until it reports them.
        #[serde(skip_serializing_if = "<[_]>::is_empty")]
        tracks: &'a [MediaTrack],
    },
    Unloaded {
        media: &'a MediaDescriptor,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a CastError>,
    },
}

/// UPnP AVTransport `TransportState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    NoMediaPresent,
    Stopped,
    Transitioning,
    Playing,
    PausedPlayback,
}

impl TransportState {
    pub fn as_str(self) -> &'static str {
        match self {
            TransportState::NoMediaPresent => "NO_MEDIA_PRESENT",
            TransportState::Stopped => "STOPPED",
            TransportState::Transitioning => "TRANSITIONING",
            TransportState::Playing => "PLAYING",
            TransportState::PausedPlayback => "PAUSED_PLAYBACK",
        }
    }
}

/// Commands from a sender: parsed from SOAP, or pressed on the receiver's
/// own screen (a TV remote, the keyboard; see [`ScreenCommand`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SenderCommand {
    SetUri(MediaDescriptor),
    Play,
    Pause,
    Stop,
    Seek {
        #[serde(rename = "toSecs", with = "crate::time::secs")]
        to: Duration,
    },
}

/// Events reported by the platform's player. Each names the load it is about.
/// JSON: `{"type":"loaded","gen":1,"durationSecs":90}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlayerEvent {
    Loaded {
        gen: Gen,
        #[serde(
            rename = "durationSecs",
            default,
            with = "crate::time::opt_secs",
            skip_serializing_if = "Option::is_none"
        )]
        duration: Option<Duration>,
    },
    LoadFailed {
        gen: Gen,
        error: CastError,
    },
    Progress {
        gen: Gen,
        #[serde(rename = "positionSecs", with = "crate::time::secs")]
        position: Duration,
        #[serde(
            rename = "durationSecs",
            default,
            with = "crate::time::opt_secs",
            skip_serializing_if = "Option::is_none"
        )]
        duration: Option<Duration>,
    },
    Ended {
        gen: Gen,
    },
    /// What the player found out about the media; may come more than once.
    Info {
        gen: Gen,
        /// Boxed: much larger than the other events.
        info: Box<MediaInfo>,
    },
    /// The player went away: its process exited or its window was closed.
    /// Nothing is loaded any more, so a Play has to load the media again.
    Closed {
        gen: Gen,
    },
    /// The tracks in the media, with the selected ones marked; again whenever they change.
    Tracks {
        gen: Gen,
        tracks: Vec<MediaTrack>,
    },
}

impl PlayerEvent {
    /// Short name for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            PlayerEvent::Loaded { .. } => "loaded",
            PlayerEvent::LoadFailed { .. } => "load_failed",
            PlayerEvent::Progress { .. } => "progress",
            PlayerEvent::Ended { .. } => "ended",
            PlayerEvent::Closed { .. } => "closed",
            PlayerEvent::Info { .. } => "info",
            PlayerEvent::Tracks { .. } => "tracks",
        }
    }

    pub fn gen(&self) -> Gen {
        match self {
            PlayerEvent::Loaded { gen, .. }
            | PlayerEvent::LoadFailed { gen, .. }
            | PlayerEvent::Progress { gen, .. }
            | PlayerEvent::Ended { gen }
            | PlayerEvent::Closed { gen }
            | PlayerEvent::Info { gen, .. }
            | PlayerEvent::Tracks { gen, .. } => *gen,
        }
    }
}

impl SenderCommand {
    /// Short name for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            SenderCommand::SetUri(_) => "set_uri",
            SenderCommand::Play => "play",
            SenderCommand::Pause => "pause",
            SenderCommand::Stop => "stop",
            SenderCommand::Seek { .. } => "seek",
        }
    }
}

/// What a press on the receiver's own screen asks for: what a phone can ask
/// (play, pause, stop, seek), and what only the screen offers.
/// JSON: `{"type":"pause"}`, `{"type":"setRate","rate":1.5}`,
/// `{"type":"selectTrack","kind":"audio","id":"2"}`, `{"type":"setSleepTimer","minutes":30}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScreenAction {
    Play,
    Pause,
    Stop,
    Seek {
        #[serde(rename = "toSecs", with = "crate::time::secs")]
        to: Duration,
    },
    /// Leave the video: back to the waiting screen (the back button).
    Close,
    /// 0..=100.
    SetVolume {
        volume: u8,
    },
    SetMuted {
        muted: bool,
    },
    /// Playback speed, within [`RATES`].
    SetRate {
        rate: f64,
    },
    /// `id: null`: subtitles off, or video back on automatic.
    SelectTrack {
        kind: TrackKind,
        #[serde(default)]
        id: Option<String>,
    },
    /// Stop after this many minutes; `null` cancels. The receiver keeps the
    /// clock (`castable-net`); the session never reads one.
    SetSleepTimer {
        #[serde(default)]
        minutes: Option<u32>,
    },
}

impl ScreenAction {
    /// The press as a phone's command, when a phone could have sent it.
    pub fn as_sender(&self) -> Option<SenderCommand> {
        Some(match self {
            ScreenAction::Play => SenderCommand::Play,
            ScreenAction::Pause => SenderCommand::Pause,
            ScreenAction::Stop => SenderCommand::Stop,
            ScreenAction::Seek { to } => SenderCommand::Seek { to: *to },
            _ => return None,
        })
    }

    /// Short name for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            ScreenAction::Play => "play",
            ScreenAction::Pause => "pause",
            ScreenAction::Stop => "stop",
            ScreenAction::Seek { .. } => "seek",
            ScreenAction::Close => "close",
            ScreenAction::SetVolume { .. } => "volume",
            ScreenAction::SetMuted { .. } => "mute",
            ScreenAction::SetRate { .. } => "rate",
            ScreenAction::SelectTrack { .. } => "track",
            ScreenAction::SetSleepTimer { .. } => "sleep_timer",
        }
    }
}

/// A press on the receiver's own screen, made while it showed cast `gen`.
/// JSON: `{"gen":1,"command":{"type":"pause"}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenCommand {
    pub gen: Gen,
    pub command: ScreenAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// From a phone. UPnP names no cast, so it acts on the current one.
    Sender(SenderCommand),
    /// From the receiver's screen; dropped if its cast has been replaced.
    Screen(ScreenCommand),
    Player(PlayerEvent),
}

/// What the platform's player must do. A `Load` always replaces whatever
/// the player had, and `Stop` also cancels a load in flight.
/// JSON: `{"type":"load","gen":1,"media":{...}}`, `{"type":"seek","toSecs":30}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlayerCommand {
    Load {
        gen: Gen,
        media: MediaDescriptor,
    },
    Play,
    Pause,
    Stop,
    Seek {
        #[serde(rename = "toSecs", with = "crate::time::secs")]
        to: Duration,
    },
    /// Volume 0..=100 (RenderingControl).
    SetVolume {
        volume: u8,
    },
    SetMuted {
        muted: bool,
    },
    /// Playback speed (1.0 is normal).
    SetRate {
        rate: f64,
    },
    /// Select the track `id` of `kind` (an id from the player's own `tracks`
    /// event); `None` turns subtitles off, or puts video back on automatic.
    SelectTrack {
        kind: TrackKind,
        #[serde(default)]
        id: Option<String>,
    },
}

/// The result of one input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub commands: Vec<PlayerCommand>,
    /// The transport state or the current media changed; senders subscribed
    /// to events should be notified.
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct Session {
    phase: Phase,
    last_gen: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self { phase: Phase::Idle, last_gen: 0 }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn cast(&self) -> Option<&Cast> {
        self.phase.cast()
    }

    pub fn transport_state(&self) -> TransportState {
        match &self.phase {
            Phase::Idle => TransportState::NoMediaPresent,
            Phase::Loading { .. } => TransportState::Transitioning,
            Phase::Loaded { playback: Playback::Playing, .. } => TransportState::Playing,
            Phase::Loaded { playback: Playback::Paused, .. } => TransportState::PausedPlayback,
            Phase::Loaded { playback: Playback::Stopped, .. } | Phase::Unloaded { .. } => TransportState::Stopped,
        }
    }

    /// A serializable snapshot for the UI (`ReceiverView` in `packages/protocol`).
    pub fn view(&self) -> View<'_> {
        let phase = match &self.phase {
            Phase::Idle => PhaseView::Idle,
            Phase::Loading { cast, then } => {
                PhaseView::Loading { media: &cast.media, play_when_ready: *then == AfterLoad::Play }
            }
            Phase::Loaded { cast, playback } => PhaseView::Loaded {
                media: &cast.media,
                playback: *playback,
                position_secs: cast.position.as_secs_f64(),
                duration_secs: cast.duration.map(|d| d.as_secs_f64()),
                info: Some(&cast.info).filter(|i| !i.is_empty()),
                quality_label: cast.info.short_label(),
                rate: cast.rate,
                tracks: &cast.tracks,
            },
            Phase::Unloaded { cast, why } => PhaseView::Unloaded {
                media: &cast.media,
                error: match why {
                    Unloaded::LoadFailed(error) => Some(error),
                    Unloaded::StoppedWhileLoading | Unloaded::PlayerClosed => None,
                },
            },
        };
        let cast = self.cast();
        View {
            transport_state: self.transport_state().as_str(),
            gen: cast.map(|c| c.gen),
            title: cast.map(|c| c.media.display_title()),
            phase,
        }
    }

    /// UPnP `TransportStatus`: `ERROR_OCCURRED` after a failed load.
    pub fn transport_status(&self) -> &'static str {
        match &self.phase {
            Phase::Unloaded { why: Unloaded::LoadFailed(_), .. } => "ERROR_OCCURRED",
            _ => "OK",
        }
    }

    pub fn handle(&mut self, input: Input) -> Result<Outcome, UpnpError> {
        let before = (self.transport_state(), self.cast().map(|c| c.gen));
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        let (phase, result) = match input {
            Input::Sender(cmd) => phase.on_sender(cmd, &mut self.last_gen),
            // A press made on a cast that has since been replaced is dropped:
            // it was meant for a video that is no longer there.
            Input::Screen(press) if phase.cast().map(|c| c.gen) != Some(press.gen) => (phase, Ok(vec![])),
            Input::Screen(press) => phase.on_screen(press.command, &mut self.last_gen),
            Input::Player(ev) => {
                let (phase, commands) = phase.on_player(ev);
                (phase, Ok(commands))
            }
        };
        // A refusal hands the phase back unchanged, so there is nothing to restore.
        self.phase = phase;
        let commands = result?;
        let after = (self.transport_state(), self.cast().map(|c| c.gen));
        Ok(Outcome { commands, changed: before != after })
    }
}

/// The transitions: pure functions from a phase and an input to the next
/// phase and what the player must do.
impl Phase {
    /// Starts loading `media` as a new generation.
    fn load(media: MediaDescriptor, then: AfterLoad, last_gen: &mut u64) -> (Phase, Vec<PlayerCommand>) {
        *last_gen += 1;
        let gen = Gen(*last_gen);
        let cast = Cast {
            gen,
            media: media.clone(),
            position: Duration::ZERO,
            duration: None,
            info: MediaInfo::default(),
            rate: 1.0,
            tracks: Vec::new(),
        };
        (Phase::Loading { cast, then }, vec![PlayerCommand::Load { gen, media }])
    }

    fn on_sender(self, cmd: SenderCommand, last_gen: &mut u64) -> (Phase, Result<Vec<PlayerCommand>, UpnpError>) {
        use PlayerCommand as P;
        use SenderCommand as S;
        let ok = |(phase, commands): (Phase, Vec<PlayerCommand>)| (phase, Ok(commands));
        let refuse = |phase: Phase| (phase, Err(UpnpError::TransitionNotAvailable));
        match (self, cmd) {
            // A new cast always replaces the old one, whatever the state.
            (_, S::SetUri(media)) => ok(Phase::load(media, AfterLoad::Hold, last_gen)),
            (Phase::Idle, _) => refuse(Phase::Idle),

            // Play: during loading it is remembered; after a failure it retries.
            (Phase::Loading { cast, .. }, S::Play) => ok((Phase::Loading { cast, then: AfterLoad::Play }, vec![])),
            (Phase::Loaded { cast, playback }, S::Play) => {
                let commands = if playback == Playback::Playing { vec![] } else { vec![P::Play] };
                ok((Phase::Loaded { cast, playback: Playback::Playing }, commands))
            }
            (Phase::Unloaded { cast, .. }, S::Play) => ok(Phase::load(cast.media, AfterLoad::Play, last_gen)),

            // Pause
            (Phase::Loading { cast, .. }, S::Pause) => ok((Phase::Loading { cast, then: AfterLoad::Hold }, vec![])),
            (Phase::Loaded { cast, playback: Playback::Playing }, S::Pause) => {
                ok((Phase::Loaded { cast, playback: Playback::Paused }, vec![P::Pause]))
            }
            (phase @ Phase::Loaded { playback: Playback::Paused, .. }, S::Pause) => ok((phase, vec![])),
            (phase @ (Phase::Loaded { .. } | Phase::Unloaded { .. }), S::Pause) => refuse(phase),

            // Stop: cancels a load; rewinds loaded media.
            (Phase::Loading { cast, .. }, S::Stop) => {
                ok((Phase::Unloaded { cast, why: Unloaded::StoppedWhileLoading }, vec![P::Stop]))
            }
            (Phase::Loaded { mut cast, playback }, S::Stop) => {
                cast.position = Duration::ZERO;
                let commands = if playback == Playback::Stopped { vec![] } else { vec![P::Stop] };
                ok((Phase::Loaded { cast, playback: Playback::Stopped }, commands))
            }
            (phase @ Phase::Unloaded { .. }, S::Stop) => ok((phase, vec![])),

            // Seek: live media can't seek. Many senders seek right after casting,
            // while it still loads: remember where to start, like a Play.
            (phase @ (Phase::Loading { .. } | Phase::Loaded { .. }), S::Seek { .. })
                if phase.cast().is_some_and(|c| c.media.is_live()) =>
            {
                (phase, Err(UpnpError::SeekModeNotSupported))
            }
            (phase @ Phase::Loaded { .. }, S::Seek { to })
                if phase.cast().and_then(|c| c.duration).is_some_and(|d| to > d) =>
            {
                (phase, Err(UpnpError::IllegalSeekTarget))
            }
            (Phase::Loaded { mut cast, playback }, S::Seek { to }) => {
                cast.position = to;
                ok((Phase::Loaded { cast, playback }, vec![P::Seek { to }]))
            }
            (Phase::Loading { mut cast, then }, S::Seek { to }) => {
                cast.position = to;
                ok((Phase::Loading { cast, then }, vec![]))
            }
            (phase @ Phase::Unloaded { .. }, S::Seek { .. }) => refuse(phase),
        }
    }

    /// A screen press. Transport presses act as a phone's would; the rest
    /// are the screen's own. Volume and the sleep timer aren't the cast's:
    /// the renderer and the receiver handle them before they get here.
    fn on_screen(self, action: ScreenAction, last_gen: &mut u64) -> (Phase, Result<Vec<PlayerCommand>, UpnpError>) {
        use ScreenAction as A;
        if let Some(cmd) = action.as_sender() {
            return self.on_sender(cmd, last_gen);
        }
        match (self, action) {
            (phase, A::Play | A::Pause | A::Stop | A::Seek { .. }) => {
                unreachable!("handled as a phone's command: {phase:?}")
            }
            // Back to waiting: whatever was loaded or loading is stopped.
            (Phase::Idle, A::Close) => (Phase::Idle, Ok(vec![])),
            (Phase::Unloaded { .. }, A::Close) => (Phase::Idle, Ok(vec![])),
            (Phase::Loading { .. } | Phase::Loaded { .. }, A::Close) => (Phase::Idle, Ok(vec![PlayerCommand::Stop])),
            (Phase::Loaded { mut cast, playback }, A::SetRate { rate }) if RATES.contains(&rate) => {
                cast.rate = rate;
                (Phase::Loaded { cast, playback }, Ok(vec![PlayerCommand::SetRate { rate }]))
            }
            (phase, A::SetRate { .. }) => (phase, Err(UpnpError::InvalidArgs)),
            (Phase::Loaded { mut cast, playback }, A::SelectTrack { kind, id }) => {
                tracks::select(&mut cast.tracks, kind, id.as_deref());
                (Phase::Loaded { cast, playback }, Ok(vec![PlayerCommand::SelectTrack { kind, id }]))
            }
            (phase, A::SelectTrack { .. }) => (phase, Err(UpnpError::TransitionNotAvailable)),
            // Not the cast's (see above): nothing for the session to do.
            (phase, A::SetVolume { .. } | A::SetMuted { .. } | A::SetSleepTimer { .. }) => (phase, Ok(vec![])),
        }
    }

    fn on_player(self, ev: PlayerEvent) -> (Phase, Vec<PlayerCommand>) {
        // Events from an older load are dropped here, in one place.
        if self.cast().map(|c| c.gen) != Some(ev.gen()) {
            return (self, vec![]);
        }
        match (self, ev) {
            (Phase::Loading { mut cast, then }, PlayerEvent::Loaded { duration, .. }) => {
                cast.duration = duration;
                if duration.is_some_and(|d| cast.position > d) {
                    cast.position = Duration::ZERO; // a seek past the end: start at the start
                }
                let mut commands = Vec::new();
                if cast.position > Duration::ZERO {
                    commands.push(PlayerCommand::Seek { to: cast.position });
                }
                let playback = match then {
                    AfterLoad::Play => {
                        commands.push(PlayerCommand::Play);
                        Playback::Playing
                    }
                    AfterLoad::Hold => Playback::Stopped,
                };
                (Phase::Loaded { cast, playback }, commands)
            }
            (Phase::Loading { cast, .. }, PlayerEvent::LoadFailed { error, .. }) => {
                (Phase::Unloaded { cast, why: Unloaded::LoadFailed(error) }, vec![])
            }
            // Media that fails mid-playback: make sure the player is stopped too.
            (Phase::Loaded { cast, .. }, PlayerEvent::LoadFailed { error, .. }) => {
                (Phase::Unloaded { cast, why: Unloaded::LoadFailed(error) }, vec![PlayerCommand::Stop])
            }
            (Phase::Loaded { mut cast, playback }, PlayerEvent::Progress { position, duration, .. }) => {
                cast.position = position;
                cast.duration = duration.or(cast.duration);
                (Phase::Loaded { cast, playback }, vec![])
            }
            // Stop too: a Play sent just before the end can reach the player
            // after it, and a browser restarts an ended video on play().
            (Phase::Loaded { mut cast, playback: Playback::Playing }, PlayerEvent::Ended { .. }) => {
                cast.position = Duration::ZERO;
                (Phase::Loaded { cast, playback: Playback::Stopped }, vec![PlayerCommand::Stop])
            }
            // Facts about the media: kept with the cast, no state change.
            (Phase::Loading { mut cast, then }, PlayerEvent::Info { info, .. }) => {
                cast.info.merge(&info);
                (Phase::Loading { cast, then }, vec![])
            }
            (Phase::Loaded { mut cast, playback }, PlayerEvent::Info { info, .. }) => {
                cast.info.merge(&info);
                (Phase::Loaded { cast, playback }, vec![])
            }
            (Phase::Loading { mut cast, then }, PlayerEvent::Tracks { tracks, .. }) => {
                cast.tracks = tracks;
                (Phase::Loading { cast, then }, vec![])
            }
            (Phase::Loaded { mut cast, playback }, PlayerEvent::Tracks { tracks, .. }) => {
                cast.tracks = tracks;
                (Phase::Loaded { cast, playback }, vec![])
            }
            // The player is gone: nothing is loaded, so Play loads it again.
            (Phase::Loading { cast, .. } | Phase::Loaded { cast, .. }, PlayerEvent::Closed { .. }) => {
                (Phase::Unloaded { cast, why: Unloaded::PlayerClosed }, vec![])
            }
            // Anything else (a duplicate event, a late progress tick) changes nothing.
            (phase, _) => (phase, vec![]),
        }
    }

    pub fn cast(&self) -> Option<&Cast> {
        match self {
            Phase::Idle => None,
            Phase::Loading { cast, .. } | Phase::Loaded { cast, .. } | Phase::Unloaded { cast, .. } => Some(cast),
        }
    }
}
