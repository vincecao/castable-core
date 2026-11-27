//! Starting, stopping and watching a player's process: shared by every backend.

use castable_net::{Gen, Handle, PlayerEvent};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::oneshot;

/// How a backend reads its process's exit.
#[derive(Debug, Clone, Copy)]
pub enum OnExit {
    /// A clean exit means the media ended; any other exit means it failed.
    EndedOrFailed,
    /// A launcher (`open -a ...`) exits at once while the app keeps playing:
    /// an exit within `grace` says nothing; a later one means it ended.
    Launcher { grace: Duration },
}

/// A running player process. Dropping it kills the process.
pub struct Proc {
    stop: Option<oneshot::Sender<()>>,
}

impl Proc {
    pub fn spawn(mut cmd: Command, gen: Gen, handle: Handle, on_exit: OnExit) -> std::io::Result<Proc> {
        let program = cmd.as_std().get_program().to_string_lossy().into_owned();
        let mut child = cmd.kill_on_drop(true).spawn()?;
        tracing::debug!(target: "player", program = %program, pid = child.id(), gen = gen.0, "started");
        let (stop, stopped) = oneshot::channel::<()>();
        let started = Instant::now();
        tokio::spawn(async move {
            tokio::select! {
                status = child.wait() => {
                    let event = match (on_exit, status) {
                        (OnExit::Launcher { grace }, Ok(s)) if s.success() && started.elapsed() < grace => None,
                        (_, Ok(s)) if s.success() => Some(PlayerEvent::Ended { gen }),
                        (_, Ok(s)) => Some(PlayerEvent::LoadFailed { gen, reason: format!("{program} exited with {s}") }),
                        (_, Err(e)) => Some(PlayerEvent::LoadFailed { gen, reason: e.to_string() }),
                    };
                    if let Some(event) = event {
                        handle.player_event(event);
                    }
                }
                // Killed by us: a new load or a stop. Report nothing.
                _ = stopped => { let _ = child.kill().await; }
            }
        });
        Ok(Proc { stop: Some(stop) })
    }

    pub async fn kill(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Reports a failed start (the program isn't installed, usually).
pub fn report_spawn_error(handle: &Handle, gen: Gen, program: &str, e: std::io::Error) {
    tracing::error!(target: "player", program, error = %e, "can't start the player; is it installed?");
    handle.player_event(PlayerEvent::LoadFailed { gen, reason: format!("{program}: {e}") });
}
