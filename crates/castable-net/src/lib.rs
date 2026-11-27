//! `castable-net`: the network layer around `castable-core`.
//!
//! [`start`] runs a DLNA MediaRenderer: an HTTP server (descriptions, SOAP
//! control, GENA events) and an SSDP agent (discovery). The platform's player
//! receives [`Command`]s from [`Receiver::commands`] and reports back through
//! [`Handle::player_event`].
//!
//! ```no_run
//! # async fn demo() -> std::io::Result<()> {
//! let mut rx = castable_net::start(castable_net::Config::named("Living room")).await?;
//! while let Some(cmd) = rx.commands.recv().await {
//!     println!("player should: {cmd:?}");
//! }
//! # Ok(()) }
//! ```

mod actor;
mod gena;
mod http;
pub mod player;
mod ssdp;

pub use castable_core::{
    CastError, DeviceInfo, ErrorCategory, ErrorCode, Gen, MediaDescriptor, PlayerCommand, PlayerEvent, ScreenCommand,
    SenderCommand,
};
pub use player::{run, Control, Player};

/// This machine's address on the network senders use (the route to the
/// SSDP multicast group), for showing users where the receiver is.
pub fn local_ip() -> Option<std::net::IpAddr> {
    ssdp::local_ip_for(ssdp::MULTICAST.into())
}

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    /// Stable device id. `None` derives one from the name, so the same name
    /// on the same machine keeps the same identity in senders' lists.
    pub uuid: Option<String>,
    /// Where the HTTP server listens. Port 0 picks a free port.
    pub http: SocketAddr,
    /// SSDP port; `None` turns discovery off (tests, or IP-only use).
    pub ssdp_port: Option<u16>,
}

impl Config {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            uuid: None,
            http: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            ssdp_port: Some(ssdp::PORT),
        }
    }
}

/// A running receiver.
pub struct Receiver {
    /// What the player must do, in order.
    pub commands: mpsc::UnboundedReceiver<PlayerCommand>,
    pub handle: Handle,
    pub http_addr: SocketAddr,
    pub device: DeviceInfo,
}

/// Cheap to clone; used by the platform to report player events and to stop.
#[derive(Clone)]
pub struct Handle {
    actor: mpsc::UnboundedSender<actor::Msg>,
    shutdown: Arc<watch::Sender<bool>>,
}

impl Handle {
    pub fn player_event(&self, event: PlayerEvent) {
        let _ = self.actor.send(actor::Msg::Player(event));
    }

    /// A press on the receiver's own screen (a button, a TV remote, a key).
    pub fn screen_command(&self, press: ScreenCommand) {
        let _ = self.actor.send(actor::Msg::Screen(press));
    }

    /// Stops the servers and sends SSDP byebye. Waits briefly for the byebye.
    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

pub async fn start(config: Config) -> std::io::Result<Receiver> {
    let uuid = config.uuid.clone().unwrap_or_else(|| {
        let host = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_default();
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, format!("{host}/{}", config.name).as_bytes()).to_string()
    });
    let device = DeviceInfo::new(&config.name, uuid);
    let (cmd_tx, commands) = mpsc::unbounded_channel();
    let actor = actor::Actor::spawn(cmd_tx);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let listener = TcpListener::bind(config.http).await?;
    let http_addr = listener.local_addr()?;
    let ctx = Arc::new(http::Ctx { device: device.clone(), actor: actor.clone() });
    tokio::spawn(serve(listener, ctx, shutdown_rx.clone()));
    if let Some(port) = config.ssdp_port {
        tokio::spawn(ssdp::run(device.clone(), http_addr.port(), port, shutdown_rx));
    }
    tracing::info!(name = %device.friendly_name, udn = %device.udn, %http_addr, "receiver started");
    Ok(Receiver { commands, handle: Handle { actor, shutdown: Arc::new(shutdown_tx) }, http_addr, device })
}

async fn serve(listener: TcpListener, ctx: Arc<http::Ctx>, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| http::handle(req, ctx.clone(), peer));
                    if let Err(e) = http1::Builder::new().serve_connection(TokioIo::new(stream), svc).await {
                        tracing::debug!(error = %e, "connection ended");
                    }
                });
            }
            _ = shutdown.changed() => return,
        }
    }
}
