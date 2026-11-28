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
pub mod profile;
mod ssdp;

pub use castable_core::device::{NameError, MAX_NAME_CHARS};
pub use castable_core::{
    AudioInfo, CastError, DeviceInfo, ErrorCategory, ErrorCode, Gen, MediaDescriptor, MediaInfo, MediaTrack,
    PlayerCommand, PlayerEvent, PlayerKey, ScreenAction, ScreenCommand, SenderCommand, TrackKind, VideoInfo,
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

/// The HTTP port a receiver asks for first, so its address stays the same
/// between runs (people type it into senders that can add a receiver by hand).
pub const PREFERRED_PORT: u16 = 49494;

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    /// The install's id (`profile::device_id`), which phones know it by. `None`
    /// derives one from the name (`profile::legacy_id`): fine for tests, but a
    /// rename then looks like a new device after a restart.
    pub uuid: Option<String>,
    /// Where the HTTP server listens. Port 0 picks a free port; any other port
    /// falls back to a free one when it's taken.
    pub http: SocketAddr,
    /// SSDP port; `None` turns discovery off (tests, or IP-only use).
    pub ssdp_port: Option<u16>,
}

impl Config {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            uuid: None,
            http: SocketAddr::from((Ipv4Addr::UNSPECIFIED, PREFERRED_PORT)),
            ssdp_port: Some(ssdp::PORT),
        }
    }
}

/// A running receiver.
pub struct Receiver {
    /// What the player must do, in order.
    pub commands: mpsc::UnboundedReceiver<PlayerCommand>,
    /// The receiver's view (`castable_core::View` as JSON: what to show), updated on every change.
    /// An app draws its screen from this; `@castable/ui` renders it as is.
    pub views: watch::Receiver<Arc<str>>,
    pub handle: Handle,
    pub http_addr: SocketAddr,
    pub device: DeviceInfo,
}

/// Cheap to clone; used by the platform to report player events and to stop.
#[derive(Clone)]
pub struct Handle {
    actor: mpsc::UnboundedSender<actor::Msg>,
    shutdown: Arc<watch::Sender<bool>>,
    device: Arc<watch::Sender<DeviceInfo>>,
}

impl Handle {
    pub fn player_event(&self, event: PlayerEvent) {
        let _ = self.actor.send(actor::Msg::Player(event));
    }

    /// A press on the receiver's own screen (a button, a TV remote, a key).
    pub fn screen_command(&self, press: ScreenCommand) {
        let _ = self.actor.send(actor::Msg::Screen(press));
    }

    /// A player key (`castable_core::keys`), acted on with what's shown now:
    /// for screens that send keys rather than build presses (the CLI).
    pub fn key(&self, key: PlayerKey) {
        let _ = self.actor.send(actor::Msg::Key(key));
    }

    /// Renames the receiver while it runs: the description serves the new name
    /// and SSDP says byebye, then alive with a new description id, so senders
    /// list it again under the new name. Its identity (UDN) stays; a cast on
    /// screen keeps playing. Returns the name as senders will show it.
    pub fn rename(&self, name: &str) -> Result<String, NameError> {
        let name = castable_core::device::friendly_name(name)?;
        self.device.send_if_modified(|device| {
            let changed = device.friendly_name != name;
            if changed {
                *device = device.renamed(name.clone());
            }
            changed
        });
        Ok(name)
    }

    /// The receiver as senders see it now (its name changes with `rename`).
    pub fn device(&self) -> DeviceInfo {
        self.device.borrow().clone()
    }

    /// Stops the servers and sends SSDP byebye. Waits briefly for the byebye.
    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

pub async fn start(config: Config) -> std::io::Result<Receiver> {
    let uuid = config.uuid.clone().unwrap_or_else(|| profile::legacy_id(&config.name));
    let name = castable_core::device::friendly_name(&config.name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let device = DeviceInfo::new(name, uuid);
    let (device_tx, device_rx) = watch::channel(device.clone());
    let (cmd_tx, commands) = mpsc::unbounded_channel();
    let (views_tx, views) = watch::channel(Arc::from(""));
    let actor = actor::Actor::spawn(cmd_tx, views_tx);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let listener = match TcpListener::bind(config.http).await {
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && config.http.port() != 0 => {
            tracing::warn!(port = config.http.port(), "HTTP port taken; using a free one");
            TcpListener::bind(SocketAddr::new(config.http.ip(), 0)).await?
        }
        bound => bound?,
    };
    let http_addr = listener.local_addr()?;
    let ctx = Arc::new(http::Ctx { device: device_rx.clone(), actor: actor.clone() });
    tokio::spawn(serve(listener, ctx, shutdown_rx.clone()));
    if let Some(port) = config.ssdp_port {
        tokio::spawn(ssdp::run(device_rx, http_addr.port(), port, shutdown_rx));
    }
    tracing::info!(name = %device.friendly_name, udn = %device.udn, %http_addr, "receiver started");
    Ok(Receiver {
        commands,
        views,
        handle: Handle { actor, shutdown: Arc::new(shutdown_tx), device: Arc::new(device_tx) },
        http_addr,
        device,
    })
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
