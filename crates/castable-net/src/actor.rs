//! One task owns the renderer and the subscriptions. Everything else talks to
//! it through [`Msg`], so there are no locks and every change is ordered.

use crate::gena::{Delivery, SubscribeRequest, Subscriptions};
use castable_core::{
    Action, PlayerCommand, PlayerEvent, Renderer, Reply, ScreenCommand, Service, TransportState, UpnpError,
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

pub type Out = Vec<(&'static str, String)>;

/// Every request the actor serves. A discriminated union: each variant
/// carries its own reply channel, typed for its answer.
#[derive(Debug)]
pub enum Msg {
    Act { action: Action, reply: oneshot::Sender<Result<Out, UpnpError>> },
    Player(PlayerEvent),
    Screen(ScreenCommand),
    Subscribe { service: Service, req: SubscribeRequest, reply: oneshot::Sender<Option<(String, Duration)>> },
    Unsubscribe { sid: String, reply: oneshot::Sender<bool> },
}

pub struct Actor {
    renderer: Renderer,
    /// The last transport state logged, so each change is logged once.
    logged_state: TransportState,
    subs: Subscriptions,
    commands: mpsc::UnboundedSender<PlayerCommand>,
    notifier: mpsc::UnboundedSender<Delivery>,
}

impl Actor {
    pub fn spawn(commands: mpsc::UnboundedSender<PlayerCommand>) -> mpsc::UnboundedSender<Msg> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (notifier, notify_rx) = mpsc::unbounded_channel();
        tokio::spawn(deliver_all(notify_rx));
        let mut actor = Actor {
            renderer: Renderer::new(),
            logged_state: TransportState::NoMediaPresent,
            subs: Subscriptions::default(),
            commands,
            notifier,
        };
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                actor.handle(msg);
            }
        });
        tx
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Act { action, reply } => {
                let name = action.name();
                if action.is_query() {
                    tracing::debug!(action = name, "control");
                } else {
                    tracing::info!(action = name, "control");
                }
                let result = self.renderer.act(action).map(|r| self.apply(r));
                if let Err(e) = &result {
                    tracing::info!(action = name, code = e.code(), error = %e, "control refused");
                }
                let _ = reply.send(result);
            }
            Msg::Player(event) => {
                let (kind, gen) = (event.kind(), event.gen().0);
                if matches!(event, PlayerEvent::Progress { .. }) {
                    tracing::trace!(event = kind, gen, "player event");
                } else if let PlayerEvent::LoadFailed { reason, .. } = &event {
                    tracing::warn!(event = kind, gen, reason = %reason, "player event");
                } else {
                    tracing::info!(event = kind, gen, "player event");
                }
                let reply = self.renderer.on_player(event);
                self.apply(reply);
            }
            Msg::Screen(press) => {
                let (command, gen) = (press.command.kind(), press.gen.0);
                tracing::info!(command, gen, "screen press");
                match self.renderer.on_screen(press) {
                    Ok(reply) => {
                        self.apply(reply);
                    }
                    Err(e) => tracing::info!(command, code = e.code(), error = %e, "screen press refused"),
                }
            }
            Msg::Subscribe { service, req, reply } => {
                let renew = matches!(req, SubscribeRequest::Renew { .. });
                let granted = self.subs.subscribe(service, req, Instant::now());
                match &granted {
                    Some((sid, timeout)) => {
                        tracing::info!(service = service.id(), sid = %sid, renew, timeout_s = timeout.as_secs(), "subscribed");
                        if !renew {
                            // The initial event, with the full state, goes to the new subscriber only.
                            self.notify(service, Some(sid));
                        }
                    }
                    None => tracing::info!(service = service.id(), renew, "subscription refused"),
                }
                let _ = reply.send(granted);
            }
            Msg::Unsubscribe { sid, reply } => {
                let removed = self.subs.unsubscribe(&sid);
                tracing::info!(sid = %sid, removed, "unsubscribed");
                let _ = reply.send(removed);
            }
        }
    }

    /// Forwards commands to the platform and events to subscribers; returns
    /// the SOAP output arguments.
    fn apply(&mut self, reply: Reply) -> Out {
        for c in reply.commands {
            log_command(&c);
            let _ = self.commands.send(c);
        }
        let state = self.renderer.session().transport_state();
        if state != self.logged_state {
            let gen = self.renderer.session().cast().map(|c| c.gen.0);
            tracing::info!(from = self.logged_state.as_str(), to = state.as_str(), gen, "transport");
            self.logged_state = state;
        }
        for service in reply.notify {
            self.notify(service, None);
        }
        reply.out
    }

    fn notify(&mut self, service: Service, only: Option<&str>) {
        let body = self.renderer.last_change(service);
        for d in self.subs.deliveries(service, only, &body, Instant::now()) {
            let _ = self.notifier.send(d);
        }
    }
}

/// One line per command, as named fields (no Debug dump of the media).
fn log_command(c: &PlayerCommand) {
    match c {
        PlayerCommand::Load { gen, media } => tracing::info!(
            command = "load",
            gen = gen.0,
            url = %media.url,
            title = media.title.as_deref().unwrap_or(""),
            mime = media.mime.as_deref().unwrap_or(""),
            live = media.is_live(),
            "player command"
        ),
        PlayerCommand::Seek { to } => tracing::info!(command = "seek", to_s = to.as_secs_f64(), "player command"),
        PlayerCommand::Play => tracing::info!(command = "play", "player command"),
        PlayerCommand::Pause => tracing::info!(command = "pause", "player command"),
        PlayerCommand::Stop => tracing::info!(command = "stop", "player command"),
        PlayerCommand::SetVolume { volume } => tracing::info!(command = "volume", volume, "player command"),
        PlayerCommand::SetMuted { muted } => tracing::info!(command = "mute", muted, "player command"),
    }
}

/// Sends NOTIFYs one at a time, so each subscriber sees events in order.
async fn deliver_all(mut rx: mpsc::UnboundedReceiver<Delivery>) {
    while let Some(d) = rx.recv().await {
        if let Err(e) = tokio::time::timeout(Duration::from_secs(3), deliver(&d))
            .await
            .unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out")))
        {
            tracing::warn!(sid = %d.sid, seq = d.seq, host = %d.callback.host, error = %e, "event delivery failed");
        }
    }
}

async fn deliver(d: &Delivery) -> std::io::Result<()> {
    let mut stream = TcpStream::connect((d.callback.host.as_str(), d.callback.port)).await?;
    stream.write_all(d.request().as_bytes()).await?;
    let mut buf = [0u8; 256];
    let _ = stream.read(&mut buf).await?; // wait for the subscriber's "200 OK"
    Ok(())
}
