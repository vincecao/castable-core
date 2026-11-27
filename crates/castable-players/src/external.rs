//! One `Player` for every registry entry: starts the program with its args,
//! and hands control to the entry's protocol.

use crate::process::{report_spawn_error, OnExit, Proc};
use crate::spec::{Control, PlayerSpec, Vars};
use crate::{mpv, vlc};
use castable_net::{player::assume_loaded, Gen, Handle, MediaDescriptor, Player, PlayerCommand};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub struct External {
    name: String,
    spec: PlayerSpec,
    program: PathBuf,
    handle: Handle,
    proc: Option<Proc>,
    /// Commands for the running player's control protocol (never `Load`).
    ctl: Option<UnboundedSender<PlayerCommand>>,
    warned: bool,
}

impl External {
    pub fn new(name: &str, spec: PlayerSpec, handle: Handle) -> Result<Self, String> {
        if !spec.runs_here() {
            return Err(format!("player {name} isn't available on {}", std::env::consts::OS));
        }
        let program =
            spec.resolve_program().ok_or_else(|| format!("player {name}: none of {:?} is installed", spec.program))?;
        Ok(Self { name: name.into(), spec, program, handle, proc: None, ctl: None, warned: false })
    }

    pub fn control(&self) -> Control {
        self.spec.control
    }

    fn send(&mut self, ctl: PlayerCommand) {
        match &self.ctl {
            Some(tx) => {
                let _ = tx.send(ctl);
            }
            None if !self.warned && self.spec.control == Control::Launch => {
                self.warned = true;
                tracing::warn!(target: "player", player = %self.name, "this player can't be controlled: play, pause and seek do nothing");
            }
            None => {}
        }
    }

    fn load(&mut self, gen: Gen, media: &MediaDescriptor) {
        let mut vars = media_vars(media);
        let socket = mpv::endpoint(&format!("{}-{}", std::process::id(), gen.0));
        let remote = vlc::Remote::new();
        match self.spec.control {
            Control::MpvIpc => {
                vars.insert("ipc", socket.display().to_string());
            }
            Control::VlcHttp => {
                vars.insert("port", remote.port.to_string());
                vars.insert("password", remote.password.clone());
            }
            Control::Launch => {}
        }
        let args = self.spec.expand_args(&vars);
        tracing::debug!(target: "player", player = %self.name, program = %self.program.display(), ?args, "launch");
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&args);
        let on_exit = match self.spec.control {
            // `open -a ...` and similar return at once while the app plays on.
            Control::Launch => OnExit::Launcher { grace: Duration::from_secs(3) },
            _ => OnExit::EndedOrFailed,
        };
        let proc = match Proc::spawn(cmd, gen, self.handle.clone(), on_exit) {
            Ok(p) => p,
            Err(e) => return report_spawn_error(&self.handle, gen, &self.name, e),
        };
        self.proc = Some(proc);
        let handle = self.handle.clone();
        match self.spec.control {
            Control::MpvIpc => {
                let (tx, rx) = unbounded_channel();
                self.ctl = Some(tx);
                tokio::spawn(mpv::session(gen, socket, rx, handle));
            }
            Control::VlcHttp => {
                let (tx, rx) = unbounded_channel();
                self.ctl = Some(tx);
                tokio::spawn(vlc::session(gen, remote, rx, handle));
            }
            Control::Launch => assume_loaded(&handle, gen),
        }
    }
}

/// The placeholders a cast fills from its media.
fn media_vars(media: &MediaDescriptor) -> Vars {
    let mut vars = Vars::new();
    vars.insert("url", media.url.clone());
    if let Some(t) = &media.title {
        vars.insert("title", t.clone());
    }
    let header = |name: &str| media.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    if let Some(r) = header("Referer") {
        vars.insert("referer", r);
    }
    if let Some(a) = header("User-Agent") {
        vars.insert("user_agent", a);
    }
    if !media.headers.is_empty() {
        let all: Vec<String> = media.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        vars.insert("headers", all.join(","));
    }
    vars
}

impl Player for External {
    async fn apply(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::Load { gen, media } => {
                self.close().await;
                self.load(gen, &media);
            }
            // A launch-only player has nothing to pause: Stop closes it.
            PlayerCommand::Stop if self.spec.control == Control::Launch => self.close().await,
            other => self.send(other),
        }
    }

    async fn close(&mut self) {
        self.ctl = None;
        if let Some(p) = self.proc.take() {
            p.kill().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_fills_its_placeholders() {
        let mut m = MediaDescriptor::video("http://x/a.m3u8");
        m.title = Some("后会友期".into());
        m.headers.insert("Referer".into(), "http://x/".into());
        let v = media_vars(&m);
        assert_eq!(v["url"], "http://x/a.m3u8");
        assert_eq!(v["title"], "后会友期");
        assert_eq!(v["referer"], "http://x/");
        assert_eq!(v["headers"], "Referer: http://x/");
        assert!(!v.contains_key("user_agent"));
    }
}
