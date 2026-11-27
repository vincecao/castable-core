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
    fn gen(&self) -> Gen {
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
        match &self.phase {
            Phase::Idle => None,
            Phase::Loading { cast, .. } | Phase::Loaded { cast, .. } | Phase::Unloaded { cast, .. } => Some(cast),
        }
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
        let commands = match input {
            Input::Sender(cmd) => self.on_sender(cmd)?,
            Input::Player(ev) => self.on_player(ev),
        };
        let after = (self.transport_state(), self.cast().map(|c| c.gen));
        Ok(Outcome { commands, changed: before != after })
    }

    fn new_gen(&mut self) -> Gen {
        self.last_gen += 1;
        Gen(self.last_gen)
    }

    /// Starts loading `media` as a new generation.
    fn load(&mut self, media: MediaDescriptor, then: AfterLoad) -> Vec<PlayerCommand> {
        let gen = self.new_gen();
        let cast = Cast { gen, media: media.clone(), position: Duration::ZERO, duration: None };
        self.phase = Phase::Loading { cast, then };
        vec![PlayerCommand::Load { gen, media }]
    }

    fn on_sender(&mut self, cmd: SenderCommand) -> Result<Vec<PlayerCommand>, UpnpError> {
        use PlayerCommand as P;
        use SenderCommand as S;
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        let (phase, commands) = match (phase, cmd) {
            // A new cast always replaces the old one, whatever the state.
            (_, S::SetUri(media)) => return Ok(self.load(media, AfterLoad::Hold)),

            (Phase::Idle, _) => {
                self.phase = Phase::Idle;
                return Err(UpnpError::TransitionNotAvailable);
            }

            // Play
            (Phase::Loading { cast, .. }, S::Play) => (Phase::Loading { cast, then: AfterLoad::Play }, vec![]),
            (Phase::Loaded { cast, playback: Playback::Playing }, S::Play) => {
                (Phase::Loaded { cast, playback: Playback::Playing }, vec![])
            }
            (Phase::Loaded { cast, .. }, S::Play) => {
                (Phase::Loaded { cast, playback: Playback::Playing }, vec![P::Play])
            }
            (Phase::Unloaded { cast, .. }, S::Play) => return Ok(self.load(cast.media, AfterLoad::Play)),

            // Pause
            (Phase::Loading { cast, .. }, S::Pause) => (Phase::Loading { cast, then: AfterLoad::Hold }, vec![]),
            (Phase::Loaded { cast, playback: Playback::Playing }, S::Pause) => {
                (Phase::Loaded { cast, playback: Playback::Paused }, vec![P::Pause])
            }
            (Phase::Loaded { cast, playback: Playback::Paused }, S::Pause) => {
                (Phase::Loaded { cast, playback: Playback::Paused }, vec![])
            }
            (phase @ (Phase::Loaded { .. } | Phase::Unloaded { .. }), S::Pause) => {
                self.phase = phase;
                return Err(UpnpError::TransitionNotAvailable);
            }

            // Stop
            (Phase::Loading { cast, .. }, S::Stop) => {
                (Phase::Unloaded { cast, why: Unloaded::StoppedWhileLoading }, vec![P::Stop])
            }
            (Phase::Loaded { mut cast, playback }, S::Stop) => {
                cast.position = Duration::ZERO;
                let cmds = if playback == Playback::Stopped { vec![] } else { vec![P::Stop] };
                (Phase::Loaded { cast, playback: Playback::Stopped }, cmds)
            }
            (phase @ Phase::Unloaded { .. }, S::Stop) => (phase, vec![]),

            // Seek
            (Phase::Loaded { mut cast, playback }, S::Seek { to }) => {
                let error = if cast.media.is_live() {
                    Some(UpnpError::SeekModeNotSupported)
                } else if cast.duration.is_some_and(|d| to > d) {
                    Some(UpnpError::IllegalSeekTarget)
                } else {
                    None
                };
                if let Some(e) = error {
                    self.phase = Phase::Loaded { cast, playback };
                    return Err(e);
                }
                cast.position = to;
                (Phase::Loaded { cast, playback }, vec![P::Seek { to }])
            }
            (phase @ (Phase::Loading { .. } | Phase::Unloaded { .. }), S::Seek { .. }) => {
                self.phase = phase;
                return Err(UpnpError::TransitionNotAvailable);
            }
        };
        self.phase = phase;
        Ok(commands)
    }

    fn on_player(&mut self, ev: PlayerEvent) -> Vec<PlayerCommand> {
        // Events from an older load are dropped here, in one place.
        if self.cast().map(|c| c.gen) != Some(ev.gen()) {
            return vec![];
        }
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        let (phase, commands) = match (phase, ev) {
            (Phase::Loading { mut cast, then }, PlayerEvent::Loaded { duration, .. }) => {
                cast.duration = duration;
                match then {
                    AfterLoad::Play => (Phase::Loaded { cast, playback: Playback::Playing }, vec![PlayerCommand::Play]),
                    AfterLoad::Hold => (Phase::Loaded { cast, playback: Playback::Stopped }, vec![]),
                }
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
                if duration.is_some() {
                    cast.duration = duration;
                }
                (Phase::Loaded { cast, playback }, vec![])
            }
            (Phase::Loaded { mut cast, playback: Playback::Playing }, PlayerEvent::Ended { .. }) => {
                cast.position = Duration::ZERO;
                (Phase::Loaded { cast, playback: Playback::Stopped }, vec![])
            }
            // Anything else (a duplicate event, a late progress tick) changes nothing.
            (phase, _) => (phase, vec![]),
        };
        self.phase = phase;
        commands
    }
}
