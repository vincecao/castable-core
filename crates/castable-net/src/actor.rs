//! One task owns the renderer and the subscriptions. Everything else talks to
//! it through [`Msg`], so there are no locks and every change is ordered.

use crate::gena::{Delivery, SubscribeRequest, Subscriptions};
use castable_core::{
    Action, ErrorCode, PlayerCommand, PlayerEvent, Renderer, Reply, ScreenCommand, Service, TransportState, UpnpError,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};

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
    /// The current view as JSON, for an app that draws the receiver's screen.
    views: watch::Sender<Arc<str>>,
}

impl Actor {
    pub fn spawn(
        commands: mpsc::UnboundedSender<PlayerCommand>,
        views: watch::Sender<Arc<str>>,
    ) -> mpsc::UnboundedSender<Msg> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (notifier, notify_rx) = mpsc::unbounded_channel();
        tokio::spawn(deliver_all(notify_rx));
        let mut actor = Actor {
            renderer: Renderer::new(),
            logged_state: TransportState::NoMediaPresent,
            subs: Subscriptions::default(),
            commands,
            notifier,
            views,
        };
        actor.publish();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                actor.handle(msg);
                actor.publish();
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
                    let code = ErrorCode::from(e);
                    tracing::info!(action = name, code = %code.id(), error = code.name(), category = code.category().name(), summary = code.summary(), upnp = e.code(), "control refused");
                }
                let _ = reply.send(result);
            }
            Msg::Player(event) => {
                let (kind, gen) = (event.kind(), event.gen().0);
                if let PlayerEvent::Progress { position, duration, .. } = &event {
                    tracing::trace!(
                        event = kind,
                        gen,
                        position_s = position.as_secs_f64(),
                        duration_s = duration.map(|d| d.as_secs_f64()),
                        "player event"
                    );
                } else if let PlayerEvent::Info { info, .. } = &event {
                    log_info(gen, info);
                } else if let PlayerEvent::LoadFailed { error, .. } = &event {
                    let c = error.code;
                    tracing::warn!(
                        event = kind,
                        gen,
                        code = %c.id(),
                        error = c.name(),
                        category = c.category().name(),
                        detail = error.detail.as_deref().unwrap_or(""),
                        summary = c.summary(),
                        "player event"
                    );
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
                    Err(e) => {
                        let code = ErrorCode::from(&e);
                        tracing::info!(command, code = %code.id(), error = code.name(), category = code.category().name(), summary = code.summary(), upnp = e.code(), "screen press refused")
                    }
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

    /// Sends the view to watchers if it changed (a phone polling its state
    /// changes nothing, so nothing is sent).
    fn publish(&self) {
        let Ok(json) = serde_json::to_string(&self.renderer.session().view()) else { return };
        self.views.send_if_modified(|current| {
            let changed = **current != *json;
            if changed {
                *current = json.into();
            }
            changed
        });
    }

    fn notify(&mut self, service: Service, only: Option<&str>) {
        let body = self.renderer.last_change(service);
        for d in self.subs.deliveries(service, only, &body, Instant::now()) {
            let _ = self.notifier.send(d);
        }
    }
}

/// One line per command, as named fields (no Debug dump of the media).
/// One line per report, every field named, plus a summary for people:
/// "1080p · 1920×1080 · H.264 · 23.98 fps · 5.2 Mb/s | AAC · 48 kHz · stereo".
fn log_info(gen: u64, i: &castable_core::MediaInfo) {
    let (v, a) = (i.video.clone().unwrap_or_default(), i.audio.clone().unwrap_or_default());
    tracing::info!(
        event = "info",
        gen,
        resolution = i.resolution_label().unwrap_or(""),
        width = v.width,
        height = v.height,
        video_codec = v.codec.as_deref().unwrap_or(""),
        fps = v.fps,
        video_bps = v.bitrate_bps,
        hdr = v.hdr.as_deref().unwrap_or(""),
        pixel_format = v.pixel_format.as_deref().unwrap_or(""),
        audio_codec = a.codec.as_deref().unwrap_or(""),
        sample_rate_hz = a.sample_rate_hz,
        channels = a.channels,
        audio_bps = a.bitrate_bps,
        container = i.container.as_deref().unwrap_or(""),
        bitrate_bps = i.bitrate_bps,
        decoder = i.decoder.as_deref().unwrap_or(""),
        summary = %i.summary(),
        "media info"
    );
}

fn log_command(c: &PlayerCommand) {
    match c {
        PlayerCommand::Load { gen, media } => tracing::info!(
            command = "load",
            gen = gen.0,
            url = %media.url,
            title = %media.display_title(),
            host = %media.host(),
            format = media.format().unwrap_or(""),
            kind = ?media.kind,
            mime = media.mime.as_deref().unwrap_or(""),
            live = media.is_live(),
            headers = media.headers.len(),
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
