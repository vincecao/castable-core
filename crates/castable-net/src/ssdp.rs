//! SSDP: answers M-SEARCH on 239.255.255.250:1900 and announces the device
//! (alive on start and every 15 minutes, byebye on shutdown). The messages
//! themselves come from `castable_core::device`.

use castable_core::device::{announcements, parse_search, search_responses, Announce, DESCRIPTION_PATH};
use castable_core::DeviceInfo;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdp};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::watch;

pub const MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
pub const PORT: u16 = 1900;
const ANNOUNCE_EVERY: Duration = Duration::from_secs(15 * 60);

/// The local address the OS would use to reach `peer`. Picks the right
/// interface on machines with several networks. No packet is sent.
pub fn local_ip_for(peer: IpAddr) -> Option<IpAddr> {
    let s = StdUdp::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((peer, PORT)).ok()?;
    Some(s.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
}

pub fn location(ip: IpAddr, http_port: u16) -> String {
    format!("http://{ip}:{http_port}{DESCRIPTION_PATH}")
}

fn bind(port: u16) -> std::io::Result<UdpSocket> {
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    s.set_reuse_port(true)?;
    s.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
    s.join_multicast_v4(&MULTICAST, &Ipv4Addr::UNSPECIFIED)?;
    s.set_multicast_loop_v4(true)?;
    s.set_nonblocking(true)?;
    UdpSocket::from_std(s.into())
}

/// A random-ish delay in `[0, max)`, so many devices don't answer at once.
fn jitter(max: Duration) -> Duration {
    let r = uuid::Uuid::new_v4().as_u128() as u64;
    Duration::from_millis(r % max.as_millis().max(1) as u64)
}

pub async fn run(device: DeviceInfo, http_port: u16, port: u16, mut shutdown: watch::Receiver<bool>) {
    let socket = match bind(port) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::warn!(error = %e, "SSDP disabled: can't bind UDP {port}; senders must use the IP directly");
            return;
        }
    };
    let device = Arc::new(device);
    let default_location = || location(local_ip_for(MULTICAST.into()).unwrap_or(Ipv4Addr::LOCALHOST.into()), http_port);
    let announce = |kind| {
        let (socket, device, loc) = (socket.clone(), device.clone(), default_location());
        async move {
            // UDP is lossy: send each announcement twice.
            for _ in 0..2 {
                for msg in announcements(&device, kind, &loc) {
                    let _ = socket.send_to(msg.as_bytes(), (MULTICAST, port)).await;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    tracing::info!(location = %default_location(), port, "ssdp up");
    announce(Announce::Alive).await;
    let mut ticker = tokio::time::interval(ANNOUNCE_EVERY);
    ticker.tick().await;
    let mut buf = vec![0u8; 2048];
    loop {
        tokio::select! {
            r = socket.recv_from(&mut buf) => {
                let Ok((n, peer)) = r else { continue };
                let Some(search) = parse_search(&String::from_utf8_lossy(&buf[..n])) else { continue };
                let ip = local_ip_for(peer.ip()).unwrap_or(Ipv4Addr::LOCALHOST.into());
                let replies = search_responses(&device, &search, &location(ip, http_port));
                if replies.is_empty() { continue }
                tracing::debug!(peer = %peer.ip(), st = %search.st, replies = replies.len(), "ssdp search");
                let socket = socket.clone();
                let delay = jitter(Duration::from_secs(u64::from(search.mx)).min(Duration::from_millis(500)));
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    for r in replies {
                        let _ = socket.send_to(r.as_bytes(), peer).await;
                    }
                });
            }
            _ = ticker.tick() => announce(Announce::Alive).await,
            _ = shutdown.changed() => {
                announce(Announce::ByeBye).await;
                tracing::info!("ssdp down");
                return;
            }
        }
    }
}
