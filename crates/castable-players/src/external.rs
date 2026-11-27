//! One `Player` for every registry entry: starts the program with its args,
//! and hands control to the entry's protocol.

use crate::process::{report_spawn_error, OnExit, Proc};
use crate::spec::{Control, PlayerSpec, Vars};
use crate::{mpv, vlc};
use castable_net::{
    player::assume_loaded, CastError, ErrorCode, Gen, Handle, MediaDescriptor, Player, PlayerCommand, PlayerEvent,
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

pub struct External {
    name: String,
    spec: PlayerSpec,
    program: PathBuf,
    handle: Handle,
    proc: Option<Proc>,
    /// `{font}`: the font for on-screen text and subtitles, if set.
    font: Option<String>,
    /// The cast loaded, if any.
    gen: Option<Gen>,
    /// Commands for the running player's control protocol (never `Load`).
    ctl: Option<UnboundedSender<PlayerCommand>>,
    warned: bool,
}

impl External {
    pub fn new(name: &str, spec: PlayerSpec, handle: Handle) -> Result<Self, CastError> {
        if !spec.runs_here() {
            let os = std::env::consts::OS;
            return Err(CastError::with_detail(
                ErrorCode::PlayerNotForThisOs,
                format!("{name} isn't available on {os}"),
            ));
        }
        let program = spec.resolve_program().ok_or_else(|| {
            CastError::with_detail(ErrorCode::PlayerMissing, format!("{name}: none of {:?} is installed", spec.program))
        })?;
        Ok(Self {
            name: name.into(),
            spec,
            program,
            handle,
            proc: None,
            font: None,
            gen: None,
            ctl: None,
            warned: false,
        })
    }

    /// Sets `{font}` (see `Settings::font`).
    pub fn with_font(mut self, font: Option<String>) -> Self {
        self.font = font;
        self
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
        if let Some(font) = &self.font {
            vars.insert("font", font.clone());
        }
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
            _ => OnExit::ClosedOrFailed,
        };
        let proc = match Proc::spawn(cmd, gen, self.handle.clone(), on_exit) {
            Ok(p) => p,
            Err(e) => return report_spawn_error(&self.handle, gen, &self.name, e),
        };
        let tail = proc.tail();
        self.proc = Some(proc);
        self.gen = Some(gen);
        let handle = self.handle.clone();
        match self.spec.control {
            Control::MpvIpc => {
                let (tx, rx) = unbounded_channel();
                self.ctl = Some(tx);
                tokio::spawn(mpv::session(gen, socket, rx, handle, tail));
            }
            Control::VlcHttp => {
                let (tx, rx) = unbounded_channel();
                self.ctl = Some(tx);
                tokio::spawn(vlc::session(gen, remote, rx, handle, tail));
            }
            Control::Launch => assume_loaded(&handle, gen),
        }
    }
}

/// The placeholders a cast fills from its media.
fn media_vars(media: &MediaDescriptor) -> Vars {
    let mut vars = Vars::new();
    vars.insert("url", media.url.clone());
    // Always a readable title: the sender's, else the decoded file name, so
    // a player never shows "%E5%90%8E%E4%BC%9A.mp4".
    vars.insert("title", media.display_title());
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
            // A launch-only player has nothing to pause: Stop closes it, and
            // says so, so that a later Play opens it again.
            PlayerCommand::Stop if self.spec.control == Control::Launch => {
                self.close().await;
                if let Some(gen) = self.gen.take() {
                    self.handle.player_event(PlayerEvent::Closed { gen });
                }
            }
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
        let mut m = MediaDescriptor::video("https://media.example.com/a.m3u8");
        m.title = Some("Sample Movie".into());
        m.headers.insert("Referer".into(), "https://media.example.com/".into());
        let v = media_vars(&m);
        assert_eq!(v["url"], "https://media.example.com/a.m3u8");
        assert_eq!(v["title"], "Sample Movie");
        assert_eq!(v["referer"], "https://media.example.com/");
        assert_eq!(v["headers"], "Referer: https://media.example.com/");
        assert!(!v.contains_key("user_agent"));
    }
}
