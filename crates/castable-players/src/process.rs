//! Starting, stopping and watching a player's process: shared by every backend.

use crate::diagnose::{classify, meaningful, Tail};
use castable_net::{CastError, ErrorCategory, ErrorCode, Gen, Handle, PlayerEvent};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

/// How a backend reads its process's exit.
#[derive(Debug, Clone, Copy)]
pub enum OnExit {
    /// A clean exit means the player closed (the video ended or you closed
    /// its window); any other exit means it failed.
    ClosedOrFailed,
    /// A launcher (`open -a ...`) exits at once while the app keeps playing:
    /// an exit within `grace` says nothing; a later one means it closed.
    Launcher { grace: Duration },
}

/// A running player process. Dropping it kills the process.
pub struct Proc {
    stop: Option<oneshot::Sender<()>>,
    tail: Tail,
}

impl Proc {
    pub fn spawn(mut cmd: Command, gen: Gen, handle: Handle, on_exit: OnExit) -> std::io::Result<Proc> {
        let program = cmd.as_std().get_program().to_string_lossy().into_owned();
        let mut child =
            cmd.kill_on_drop(true).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        tracing::debug!(target: "player", program = %program, pid = child.id(), gen = gen.0, "started");
        // Keep the player's last lines: they say why it failed ("HTTP error
        // 403 Forbidden"), which its exit code alone doesn't.
        let tail = Tail::default();
        if let Some(out) = child.stdout.take() {
            tokio::spawn(read_output(out, tail.clone()));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(read_output(err, tail.clone()));
        }
        let watched = tail.clone();
        let (stop, stopped) = oneshot::channel::<()>();
        let started = Instant::now();
        tokio::spawn(async move {
            tokio::select! {
                status = child.wait() => {
                    // Let the readers take the last lines first.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let tail = watched;
                    let event = match (on_exit, status) {
                        (OnExit::Launcher { grace }, Ok(s)) if s.success() && started.elapsed() < grace => None,
                        // A clean exit that printed a network or media error (VLC exits 0
                        // when it can't open a stream) is a failure, not a close.
                        (_, Ok(s)) if s.success() => match tail.diagnose(ErrorCode::Unknown) {
                            e if matches!(e.code.category(), ErrorCategory::Network | ErrorCategory::Media) => {
                                Some(PlayerEvent::LoadFailed { gen, error: e })
                            }
                            _ => Some(PlayerEvent::Closed { gen }),
                        },
                        (_, Ok(s)) => {
                            let mut error = tail.diagnose(ErrorCode::PlayerCrashed);
                            let exit = format!("{program} exited with {s}");
                            error.detail = Some(error.detail.map_or(exit.clone(), |d| format!("{d} ({exit})")));
                            Some(PlayerEvent::LoadFailed { gen, error })
                        }
                        (_, Err(e)) => Some(PlayerEvent::LoadFailed { gen, error: CastError::with_detail(ErrorCode::PlayerCrashed, e.to_string()) }),
                    };
                    if let Some(event) = event {
                        handle.player_event(event);
                    }
                }
                // Killed by us: a new load or a stop. Report nothing.
                _ = stopped => { let _ = child.kill().await; }
            }
        });
        Ok(Proc { stop: Some(stop), tail })
    }

    /// The player's last output lines, for error reports.
    pub fn tail(&self) -> Tail {
        self.tail.clone()
    }

    pub async fn kill(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Reads a player's output line by line: each meaningful line is logged at
/// debug (with its error code, if it names one) and kept in `tail`.
async fn read_output(stream: impl AsyncRead + Unpin, tail: Tail) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Some(line) = meaningful(&line) else { continue };
        match classify(&line) {
            Some(code) => {
                tracing::debug!(target: "player", output = %line, code = %code.id(), error = code.name(), "player says")
            }
            None => tracing::debug!(target: "player", output = %line, "player says"),
        }
        tail.push(line);
    }
}

/// Reports a failed start (the program isn't installed, usually).
pub fn report_spawn_error(handle: &Handle, gen: Gen, program: &str, e: std::io::Error) {
    let error = CastError::with_detail(ErrorCode::PlayerMissing, format!("{program}: {e}"));
    tracing::error!(target: "player", code = %error.code.id(), error = error.code.name(), detail = %e, program, "can't start the player; is it installed?");
    handle.player_event(PlayerEvent::LoadFailed { gen, error });
}
