//! GENA event subscriptions (UPnP eventing): SUBSCRIBE, renew, UNSUBSCRIBE,
//! and the NOTIFY requests sent to subscribers.
//!
//! [`Subscriptions`] is plain data with the clock passed in, so it is tested
//! without sockets or sleeping.

use castable_core::Service;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1800);
const MIN_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_TIMEOUT: Duration = Duration::from_secs(7200);
/// At most this many live subscriptions, and this many from one address, so
/// one host on the network can't grow the table (and every NOTIFY fan-out)
/// without bound. A phone needs one per service.
pub const MAX_SUBSCRIPTIONS: usize = 32;
pub const MAX_PER_HOST: usize = 8;

/// Why a SUBSCRIBE was refused (all answered 412 Precondition Failed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// A renewal named a SID that isn't (or is no longer) subscribed.
    UnknownSid,
    /// The CALLBACK isn't the subscriber's own address. Sending NOTIFYs
    /// elsewhere would let any host aim the receiver at other machines or its
    /// own localhost services (CallStranger, CVE-2020-12695).
    NotYourAddress,
    /// Too many subscriptions, in all or from this address.
    TooMany,
}

impl Refused {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownSid => "unknown_sid",
            Self::NotYourAddress => "callback_not_subscriber",
            Self::TooMany => "too_many",
        }
    }
}

/// A subscriber's callback, parsed from `CALLBACK: <http://host:port/path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Callback {
    /// Takes the first `http://` URL from a CALLBACK header.
    pub fn parse(header: &str) -> Option<Callback> {
        let url = header.split('>').find_map(|part| part.trim().strip_prefix('<'))?;
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()?),
            None => (authority, 80),
        };
        (!host.is_empty()).then(|| Callback { host: host.to_string(), port, path: path.to_string() })
    }
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub sid: String,
    pub service: Service,
    pub callback: Callback,
    pub expires: Instant,
    seq: u32,
}

impl Subscription {
    /// The next event sequence number: 0 first, then 1.. wrapping back to 1.
    fn next_seq(&mut self) -> u32 {
        let seq = self.seq;
        self.seq = if self.seq == u32::MAX { 1 } else { self.seq + 1 };
        seq
    }
}

/// What a SUBSCRIBE request asks for: a new subscription or a renewal.
/// Spelled as a union so a request can't be both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscribeRequest {
    New { callback: Callback, timeout: Duration },
    Renew { sid: String, timeout: Duration },
}

impl SubscribeRequest {
    /// Reads the SUBSCRIBE headers. `None` means 412 Precondition Failed.
    pub fn from_headers(
        callback: Option<&str>,
        nt: Option<&str>,
        sid: Option<&str>,
        timeout: Option<&str>,
    ) -> Option<Self> {
        let timeout = parse_timeout(timeout);
        match (sid, callback) {
            (Some(sid), None) => Some(Self::Renew { sid: sid.trim().to_string(), timeout }),
            (None, Some(cb)) if nt.map(str::trim) == Some("upnp:event") => {
                Some(Self::New { callback: Callback::parse(cb)?, timeout })
            }
            _ => None,
        }
    }
}

/// `Second-1800` → 1800 s, clamped; `infinite` or missing → the default.
pub fn parse_timeout(header: Option<&str>) -> Duration {
    header
        .and_then(|h| h.trim().strip_prefix("Second-"))
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT)
        .clamp(MIN_TIMEOUT, MAX_TIMEOUT)
}

/// An event to deliver: everything needed to write one NOTIFY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub sid: String,
    pub seq: u32,
    pub callback: Callback,
    pub body: String,
}

impl Delivery {
    pub fn request(&self) -> String {
        format!(
            "NOTIFY {path} HTTP/1.1\r\nHOST: {host}:{port}\r\nCONTENT-TYPE: text/xml; charset=\"utf-8\"\r\nNT: upnp:event\r\nNTS: upnp:propchange\r\nSID: {sid}\r\nSEQ: {seq}\r\nCONTENT-LENGTH: {len}\r\nCONNECTION: close\r\n\r\n{body}",
            path = self.callback.path,
            host = self.callback.host,
            port = self.callback.port,
            sid = self.sid,
            seq = self.seq,
            len = self.body.len(),
            body = self.body,
        )
    }
}

#[derive(Debug, Default)]
pub struct Subscriptions {
    by_sid: HashMap<String, Subscription>,
}

impl Subscriptions {
    /// Adds a subscription for `peer` (the address the request came from) or
    /// renews one. Returns the SID and granted timeout, or why it was refused.
    pub fn subscribe(
        &mut self,
        service: Service,
        req: SubscribeRequest,
        peer: IpAddr,
        now: Instant,
    ) -> Result<(String, Duration), Refused> {
        match req {
            SubscribeRequest::New { callback, timeout } => {
                let peer = peer.to_canonical();
                if callback.host.parse::<IpAddr>().map(|ip| ip.to_canonical()) != Ok(peer) {
                    return Err(Refused::NotYourAddress);
                }
                self.by_sid.retain(|_, s| s.expires > now);
                let from_peer = self.by_sid.values().filter(|s| s.callback.host == callback.host).count();
                if self.by_sid.len() >= MAX_SUBSCRIPTIONS || from_peer >= MAX_PER_HOST {
                    return Err(Refused::TooMany);
                }
                let sid = format!("uuid:{}", uuid::Uuid::new_v4());
                let sub = Subscription { sid: sid.clone(), service, callback, expires: now + timeout, seq: 0 };
                self.by_sid.insert(sid.clone(), sub);
                Ok((sid, timeout))
            }
            SubscribeRequest::Renew { sid, timeout } => {
                let sub = self
                    .by_sid
                    .get_mut(&sid)
                    .filter(|s| s.service == service && s.expires > now)
                    .ok_or(Refused::UnknownSid)?;
                sub.expires = now + timeout;
                Ok((sid, timeout))
            }
        }
    }

    pub fn unsubscribe(&mut self, sid: &str) -> bool {
        self.by_sid.remove(sid).is_some()
    }

    /// Builds one delivery per live subscriber of `service` (or only `sid`),
    /// dropping expired subscriptions on the way.
    pub fn deliveries(&mut self, service: Service, only: Option<&str>, body: &str, now: Instant) -> Vec<Delivery> {
        self.by_sid.retain(|_, s| s.expires > now);
        self.by_sid
            .values_mut()
            .filter(|s| s.service == service && only.is_none_or(|sid| s.sid == sid))
            .map(|s| Delivery {
                sid: s.sid.clone(),
                seq: s.next_seq(),
                callback: s.callback.clone(),
                body: body.to_string(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 20));

    #[test]
    fn a_callback_must_be_the_subscribers_own_address() {
        let mut subs = Subscriptions::default();
        let t0 = Instant::now();
        let elsewhere = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 99));
        assert_eq!(subs.subscribe(Service::AvTransport, new_req(), elsewhere, t0), Err(Refused::NotYourAddress));
        let localhost =
            SubscribeRequest::from_headers(Some("<http://127.0.0.1:8080/>"), Some("upnp:event"), None, None).unwrap();
        assert_eq!(subs.subscribe(Service::AvTransport, localhost, PEER, t0), Err(Refused::NotYourAddress));
        // An IPv4 peer seen as IPv4-mapped IPv6 is the same address.
        let mapped = IpAddr::V6(std::net::Ipv4Addr::new(192, 0, 2, 20).to_ipv6_mapped());
        assert!(subs.subscribe(Service::AvTransport, new_req(), mapped, t0).is_ok());
    }

    #[test]
    fn subscriptions_are_capped_per_host_and_in_all() {
        let mut subs = Subscriptions::default();
        let t0 = Instant::now();
        for _ in 0..MAX_PER_HOST {
            assert!(subs.subscribe(Service::AvTransport, new_req(), PEER, t0).is_ok());
        }
        assert_eq!(subs.subscribe(Service::AvTransport, new_req(), PEER, t0), Err(Refused::TooMany));
        // Expired ones don't count.
        assert!(subs.subscribe(Service::AvTransport, new_req(), PEER, t0 + Duration::from_secs(400)).is_ok());
        let mut all = Subscriptions::default();
        for i in 0..MAX_SUBSCRIPTIONS {
            let host = format!("192.0.2.{}", 100 + i);
            let req =
                SubscribeRequest::from_headers(Some(&format!("<http://{host}:4000/>")), Some("upnp:event"), None, None)
                    .unwrap();
            assert!(all.subscribe(Service::AvTransport, req, host.parse().unwrap(), t0).is_ok());
        }
        assert_eq!(all.subscribe(Service::AvTransport, new_req(), PEER, t0), Err(Refused::TooMany));
    }

    fn new_req() -> SubscribeRequest {
        SubscribeRequest::from_headers(
            Some("<http://192.0.2.20:49999/evt/cb>"),
            Some("upnp:event"),
            None,
            Some("Second-300"),
        )
        .unwrap()
    }

    #[test]
    fn parses_callbacks() {
        assert_eq!(
            Callback::parse("<http://10.0.0.2:5000/x/y><http://other/>"),
            Some(Callback { host: "10.0.0.2".into(), port: 5000, path: "/x/y".into() })
        );
        assert_eq!(Callback::parse("<http://h>").unwrap().port, 80);
        assert_eq!(Callback::parse("<https://h/>"), None);
    }

    #[test]
    fn headers_pick_new_or_renew() {
        assert!(matches!(new_req(), SubscribeRequest::New { timeout, .. } if timeout == Duration::from_secs(300)));
        let renew = SubscribeRequest::from_headers(None, None, Some("uuid:abc"), Some("Second-infinite"));
        assert_eq!(renew, Some(SubscribeRequest::Renew { sid: "uuid:abc".into(), timeout: DEFAULT_TIMEOUT }));
        // Both SID and CALLBACK, or a wrong NT: precondition failed.
        assert_eq!(SubscribeRequest::from_headers(Some("<http://h/>"), Some("upnp:event"), Some("uuid:x"), None), None);
        assert_eq!(SubscribeRequest::from_headers(Some("<http://h/>"), Some("other"), None, None), None);
    }

    #[test]
    fn sequence_renewal_and_expiry() {
        let t0 = Instant::now();
        let mut subs = Subscriptions::default();
        let (sid, _) = subs.subscribe(Service::AvTransport, new_req(), PEER, t0).unwrap();
        let first = subs.deliveries(Service::AvTransport, None, "<e/>", t0);
        let second = subs.deliveries(Service::AvTransport, None, "<e/>", t0);
        assert_eq!((first[0].seq, second[0].seq), (0, 1));
        assert!(subs.deliveries(Service::RenderingControl, None, "<e/>", t0).is_empty());

        let renew = SubscribeRequest::Renew { sid: sid.clone(), timeout: Duration::from_secs(300) };
        assert!(subs.subscribe(Service::AvTransport, renew.clone(), PEER, t0 + Duration::from_secs(200)).is_ok());
        assert_eq!(subs.deliveries(Service::AvTransport, None, "<e/>", t0 + Duration::from_secs(400)).len(), 1);
        assert!(subs.deliveries(Service::AvTransport, None, "<e/>", t0 + Duration::from_secs(600)).is_empty());
        assert_eq!(
            subs.subscribe(Service::AvTransport, renew, PEER, t0 + Duration::from_secs(600)),
            Err(Refused::UnknownSid)
        );
    }

    #[test]
    fn notify_request_is_well_formed() {
        let d = Delivery {
            sid: "uuid:1".into(),
            seq: 3,
            callback: Callback { host: "h".into(), port: 9, path: "/cb".into() },
            body: "<x/>".into(),
        };
        let r = d.request();
        assert!(r.starts_with("NOTIFY /cb HTTP/1.1\r\nHOST: h:9\r\n"));
        assert!(r.contains("SEQ: 3\r\n") && r.ends_with("CONNECTION: close\r\n\r\n<x/>"));
    }
}
