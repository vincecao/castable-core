//! The player keys every Castable screen shares: the apps' UI (`usePlayerKeys`
//! in castable-universal) and the CLI's status screen. One table, so a key
//! does the same thing everywhere:
//!
//! | Key | Does |
//! | --- | --- |
//! | `play-pause` (Space, K) | play or pause |
//! | `seek-back` / `seek-forward` (← →) | 5 s |
//! | `step-back` / `step-forward` (Shift ← →) | 1 s |
//! | `jump-back` / `jump-forward` (J L) | 10 s |
//! | `volume-up` / `volume-down` (↑ ↓) | ±5 |
//! | `mute` (M) | mute or unmute |
//! | `slower` / `faster` (< >) | the next speed in [`SPEEDS`] |
//! | `back` (Esc) | leave the video, back to waiting |
//!
//! The UI builds the same actions itself (it shows them ahead of the view);
//! this is for screens that only send keys.

use crate::renderer::Renderer;
use crate::session::{Phase, Playback, ScreenAction};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Seconds the seek keys move.
pub const SEEK_STEP: f64 = 5.0;
pub const SEEK_FINE: f64 = 1.0;
pub const SEEK_LONG: f64 = 10.0;
/// Points the volume keys move.
pub const VOLUME_STEP: u8 = 5;
/// The speeds the speed keys step through, slowest first.
pub const SPEEDS: [f64; 8] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0];

/// JSON (and the CLI's control lines): `"play-pause"`, `"seek-back"`, ...
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayerKey {
    PlayPause,
    SeekBack,
    SeekForward,
    StepBack,
    StepForward,
    JumpBack,
    JumpForward,
    VolumeUp,
    VolumeDown,
    Mute,
    Slower,
    Faster,
    Back,
}

impl PlayerKey {
    /// `"seek-back"` → `SeekBack`.
    pub fn parse(name: &str) -> Option<Self> {
        use serde::de::IntoDeserializer;
        let name: serde::de::value::StrDeserializer<'_, serde::de::value::Error> = name.trim().into_deserializer();
        Self::deserialize(name).ok()
    }
}

impl Renderer {
    /// What `key` does now, as a screen action; `None` when it does nothing
    /// (nothing cast, seeking live media, already at the slowest speed...).
    pub fn key_action(&self, key: PlayerKey) -> Option<ScreenAction> {
        use PlayerKey as K;
        let (volume, muted) = self.volume();
        // Volume and back don't need loaded media.
        match key {
            K::VolumeUp => {
                return Some(ScreenAction::SetVolume { volume: volume.saturating_add(VOLUME_STEP).min(100) })
            }
            K::VolumeDown => return Some(ScreenAction::SetVolume { volume: volume.saturating_sub(VOLUME_STEP) }),
            K::Mute => return Some(ScreenAction::SetMuted { muted: !muted }),
            K::Back => return self.session().cast().map(|_| ScreenAction::Close),
            _ => {}
        }
        let Phase::Loaded { cast, playback } = self.session().phase() else {
            // Play still works while loading or after a failure: the session remembers or retries.
            return matches!(key, K::PlayPause)
                .then_some(ScreenAction::Play)
                .filter(|_| self.session().cast().is_some());
        };
        let seek = |by: f64| {
            if cast.media.is_live() {
                return None;
            }
            // A duration of 0 means the player doesn't know it yet (a stream), not "ends at 0".
            let end = cast.duration.filter(|d| !d.is_zero()).map_or(f64::MAX, |d| d.as_secs_f64());
            let to = (cast.position.as_secs_f64() + by).clamp(0.0, end);
            Some(ScreenAction::Seek { to: Duration::from_secs_f64(to) })
        };
        // The next listed speed strictly above (faster) or below (slower) the
        // current one, which a phone may have set to a speed not in the list.
        let faster = || SPEEDS.iter().find(|s| **s > cast.rate + 1e-3).map(|&rate| ScreenAction::SetRate { rate });
        let slower =
            || SPEEDS.iter().rev().find(|s| **s < cast.rate - 1e-3).map(|&rate| ScreenAction::SetRate { rate });
        match key {
            K::PlayPause if *playback == Playback::Playing => Some(ScreenAction::Pause),
            K::PlayPause => Some(ScreenAction::Play),
            K::SeekBack => seek(-SEEK_STEP),
            K::SeekForward => seek(SEEK_STEP),
            K::StepBack => seek(-SEEK_FINE),
            K::StepForward => seek(SEEK_FINE),
            K::JumpBack => seek(-SEEK_LONG),
            K::JumpForward => seek(SEEK_LONG),
            K::Slower => slower(),
            K::Faster => faster(),
            K::VolumeUp | K::VolumeDown | K::Mute | K::Back => unreachable!("handled above"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::Action;
    use crate::session::{Gen, PlayerEvent, ScreenCommand};

    fn playing() -> Renderer {
        let mut r = Renderer::new();
        r.act(Action::SetUri { uri: "http://192.0.2.10/e1.mp4".into(), metadata: String::new() }).unwrap();
        r.act(Action::Play).unwrap();
        r.on_player(PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(60)) });
        r.on_player(PlayerEvent::Progress { gen: Gen(1), position: Duration::from_secs(3), duration: None });
        r
    }

    #[test]
    fn keys_have_names() {
        assert_eq!(PlayerKey::parse("seek-back"), Some(PlayerKey::SeekBack));
        assert_eq!(PlayerKey::parse(" play-pause\n"), Some(PlayerKey::PlayPause));
        assert_eq!(PlayerKey::parse("dance"), None);
    }

    #[test]
    fn keys_act_on_what_is_shown() {
        let mut r = playing();
        assert_eq!(r.key_action(PlayerKey::PlayPause), Some(ScreenAction::Pause));
        // Seeks stop at the start.
        assert_eq!(r.key_action(PlayerKey::SeekBack), Some(ScreenAction::Seek { to: Duration::ZERO }));
        assert_eq!(r.key_action(PlayerKey::StepForward), Some(ScreenAction::Seek { to: Duration::from_secs(4) }));
        assert_eq!(r.key_action(PlayerKey::VolumeUp), Some(ScreenAction::SetVolume { volume: 55 }));
        assert_eq!(r.key_action(PlayerKey::Faster), Some(ScreenAction::SetRate { rate: 1.25 }));
        r.on_screen(ScreenCommand { gen: Gen(1), command: ScreenAction::SetRate { rate: 0.5 } }).unwrap();
        assert_eq!(r.key_action(PlayerKey::Slower), None, "already the slowest");
        assert_eq!(r.key_action(PlayerKey::Back), Some(ScreenAction::Close));
    }

    #[test]
    fn speed_keys_step_from_speeds_not_in_the_list() {
        let mut r = playing();
        let set = |r: &mut Renderer, rate| {
            r.on_screen(ScreenCommand { gen: Gen(1), command: ScreenAction::SetRate { rate } }).unwrap()
        };
        set(&mut r, 4.0);
        assert_eq!(r.key_action(PlayerKey::Faster), None, "already above the list");
        assert_eq!(r.key_action(PlayerKey::Slower), Some(ScreenAction::SetRate { rate: 3.0 }));
        set(&mut r, 0.25);
        assert_eq!(r.key_action(PlayerKey::Slower), None);
        assert_eq!(r.key_action(PlayerKey::Faster), Some(ScreenAction::SetRate { rate: 0.5 }));
        set(&mut r, 1.1);
        assert_eq!(r.key_action(PlayerKey::Faster), Some(ScreenAction::SetRate { rate: 1.25 }));
        assert_eq!(r.key_action(PlayerKey::Slower), Some(ScreenAction::SetRate { rate: 1.0 }));
    }

    #[test]
    fn an_unknown_length_doesnt_pin_seeks_to_zero() {
        let mut r = playing();
        r.on_player(PlayerEvent::Progress {
            gen: Gen(1),
            position: Duration::from_secs(30),
            duration: Some(Duration::ZERO),
        });
        assert_eq!(r.key_action(PlayerKey::SeekForward), Some(ScreenAction::Seek { to: Duration::from_secs(35) }));
    }

    #[test]
    fn nothing_cast_means_only_the_volume() {
        let r = Renderer::new();
        assert_eq!(r.key_action(PlayerKey::PlayPause), None);
        assert_eq!(r.key_action(PlayerKey::Back), None);
        assert_eq!(r.key_action(PlayerKey::Mute), Some(ScreenAction::SetMuted { muted: true }));
    }
}
