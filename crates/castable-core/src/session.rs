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
}

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
    LoadFailed(String),
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

/// What the UI renders. Mirrors `ReceiverView` in `packages/descriptor`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View<'a> {
    pub transport_state: &'static str,
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
    },
    Unloaded {
        media: &'a MediaDescriptor,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a str>,
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

/// Commands from a sender: parsed from SOAP, or sent by the receiver's own
/// UI (a TV remote, the keyboard) as JSON.
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
        reason: String,
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
}

impl PlayerEvent {
    /// Short name for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            PlayerEvent::Loaded { .. } => "loaded",
            PlayerEvent::LoadFailed { .. } => "load_failed",
            PlayerEvent::Progress { .. } => "progress",
            PlayerEvent::Ended { .. } => "ended",
        }
    }

    pub fn gen(&self) -> Gen {
        match self {
            PlayerEvent::Loaded { gen, .. }
            | PlayerEvent::LoadFailed { gen, .. }
            | PlayerEvent::Progress { gen, .. }
            | PlayerEvent::Ended { gen } => *gen,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Sender(SenderCommand),
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

    /// A serializable snapshot for the UI (`packages/descriptor` `ReceiverView`).
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
            },
            Phase::Unloaded { cast, why } => PhaseView::Unloaded {
                media: &cast.media,
                error: match why {
                    Unloaded::LoadFailed(reason) => Some(reason),
                    Unloaded::StoppedWhileLoading => None,
                },
            },
        };
        View { transport_state: self.transport_state().as_str(), phase }
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
        let cast = Cast { gen, media: media.clone(), position: Duration::ZERO, duration: None };
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
            (Phase::Loading { cast, .. }, PlayerEvent::LoadFailed { reason, .. }) => {
                (Phase::Unloaded { cast, why: Unloaded::LoadFailed(reason) }, vec![])
            }
            // Media that fails mid-playback: make sure the player is stopped too.
            (Phase::Loaded { cast, .. }, PlayerEvent::LoadFailed { reason, .. }) => {
                (Phase::Unloaded { cast, why: Unloaded::LoadFailed(reason) }, vec![PlayerCommand::Stop])
            }
            (Phase::Loaded { mut cast, playback }, PlayerEvent::Progress { position, duration, .. }) => {
                cast.position = position;
                cast.duration = duration.or(cast.duration);
                (Phase::Loaded { cast, playback }, vec![])
            }
            (Phase::Loaded { mut cast, playback: Playback::Playing }, PlayerEvent::Ended { .. }) => {
                cast.position = Duration::ZERO;
                (Phase::Loaded { cast, playback: Playback::Stopped }, vec![])
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
