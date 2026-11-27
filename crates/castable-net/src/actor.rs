//! One task owns the renderer and the subscriptions. Everything else talks to
//! it through [`Msg`], so there are no locks and every change is ordered.

use crate::gena::{Delivery, SubscribeRequest, Subscriptions};
use castable_core::{Action, Command, PlayerEvent, Renderer, Reply, Service, UpnpError};
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
    Subscribe { service: Service, req: SubscribeRequest, reply: oneshot::Sender<Option<(String, Duration)>> },
    Unsubscribe { sid: String, reply: oneshot::Sender<bool> },
}

pub struct Actor {
    renderer: Renderer,
    subs: Subscriptions,
    commands: mpsc::UnboundedSender<Command>,
    notifier: mpsc::UnboundedSender<Delivery>,
}

impl Actor {
    pub fn spawn(commands: mpsc::UnboundedSender<Command>) -> mpsc::UnboundedSender<Msg> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (notifier, notify_rx) = mpsc::unbounded_channel();
        tokio::spawn(deliver_all(notify_rx));
        let mut actor = Actor { renderer: Renderer::new(), subs: Subscriptions::default(), commands, notifier };
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
                tracing::debug!(?action, "action");
                let result = self.renderer.act(action).map(|r| self.apply(r));
                let _ = reply.send(result);
            }
            Msg::Player(event) => {
                tracing::debug!(?event, "player event");
                let reply = self.renderer.on_player(event);
                self.apply(reply);
            }
            Msg::Subscribe { service, req, reply } => {
                let granted = self.subs.subscribe(service, req, Instant::now());
                if let Some((sid, _)) = &granted {
                    // The initial event, with the full state, goes to the new subscriber only.
                    self.notify(service, Some(sid));
                }
                let _ = reply.send(granted);
            }
            Msg::Unsubscribe { sid, reply } => {
                let _ = reply.send(self.subs.unsubscribe(&sid));
            }
        }
    }

    /// Forwards commands to the platform and events to subscribers; returns
    /// the SOAP output arguments.
    fn apply(&mut self, reply: Reply) -> Out {
        for c in reply.commands {
            tracing::info!(command = ?c, "player command");
            let _ = self.commands.send(c);
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

/// Sends NOTIFYs one at a time, so each subscriber sees events in order.
async fn deliver_all(mut rx: mpsc::UnboundedReceiver<Delivery>) {
    while let Some(d) = rx.recv().await {
        if let Err(e) = tokio::time::timeout(Duration::from_secs(3), deliver(&d))
            .await
            .unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out")))
        {
            tracing::warn!(sid = %d.sid, error = %e, "event delivery failed");
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
