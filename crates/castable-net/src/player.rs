//! The seam between the receiver and whatever plays the media.
//!
//! A player gets [`PlayerCommand`]s and reports [`PlayerEvent`]s through the
//! [`Handle`]. It never decides anything: the core already did. Each event
//! names the load (`gen`) it is about, so a late or duplicate event is
//! harmless: the core drops it.
//!
//! Implement [`Player`] for a new backend and drive it with [`run`].

use crate::{Handle, PlayerCommand, PlayerEvent, Receiver};
use std::future::Future;

pub trait Player: Send {
    /// Carry out one command. Commands arrive in order; a `Load` replaces
    /// whatever was playing, and `Stop` also cancels a load in flight.
    /// Volume and mute may be ignored by players that can't set them.
    fn apply(&mut self, command: PlayerCommand) -> impl Future<Output = ()> + Send;

    /// Release the player (stop it, close windows) before the receiver exits.
    fn close(&mut self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// What a player can do, so the receiver can say so in its logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Play, pause, seek and stop work, and the player reports load, progress and end.
    Full,
    /// The player is only started and stopped; play, pause and seek do nothing.
    LaunchOnly,
}

/// Feeds the receiver's commands to `player` until `until` resolves (for
/// example Ctrl-C) or the receiver stops, then closes the player and the
/// receiver (SSDP byebye).
pub async fn run<P: Player>(rx: Receiver, mut player: P, until: impl Future<Output = ()>) {
    let Receiver { mut commands, handle, .. } = rx;
    tokio::pin!(until);
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(c) => player.apply(c).await,
                None => break,
            },
            _ = &mut until => break,
        }
    }
    player.close().await;
    handle.shutdown().await;
}

/// Reports `Loaded` at once: for players that can't say when media is ready.
pub fn assume_loaded(handle: &Handle, gen: crate::Gen) {
    handle.player_event(PlayerEvent::Loaded { gen, duration: None });
}
