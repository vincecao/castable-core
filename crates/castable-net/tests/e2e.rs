//! End-to-end over loopback: a sender's full conversation with the receiver,
//! speaking raw HTTP and UDP like a phone app would.

use castable_net::{Config, Gen, PlayerCommand, PlayerEvent};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

struct HttpResponse {
    status: u16,
    head: String,
    body: String,
}

async fn http(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        req += &format!("{k}: {v}\r\n");
    }
    req += "\r\n";
    req += body;
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    HttpResponse { status, head: head.to_string(), body: body.to_string() }
}

fn soap(action: &str, args: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action} xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>0</InstanceID>{args}</u:{action}></s:Body></s:Envelope>"#
    )
}

async fn avt(addr: SocketAddr, action: &str, args: &str) -> HttpResponse {
    let sa = format!("\"urn:schemas-upnp-org:service:AVTransport:1#{action}\"");
    http(addr, "POST", "/ctl/avt", &[("SOAPACTION", &sa), ("Content-Type", "text/xml")], &soap(action, args)).await
}

/// Accepts one NOTIFY on the subscriber's callback and answers 200.
async fn next_event(events: &TcpListener) -> String {
    let (mut s, _) = tokio::time::timeout(Duration::from_secs(3), events.accept()).await.expect("no event").unwrap();
    let mut buf = vec![0u8; 16384];
    let mut raw = String::new();
    while !raw.contains("</e:propertyset>") {
        let n = s.read(&mut buf).await.unwrap();
        assert!(n > 0, "event connection closed early");
        raw.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
    raw
}

async fn next_command(rx: &mut tokio::sync::mpsc::UnboundedReceiver<PlayerCommand>) -> PlayerCommand {
    tokio::time::timeout(Duration::from_secs(3), rx.recv()).await.expect("no command").unwrap()
}

fn local_config() -> Config {
    Config { http: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), ssdp_port: None, ..Config::named("Test receiver") }
}

#[tokio::test]
async fn a_phone_casts_plays_and_gets_events() {
    let mut rx = castable_net::start(local_config()).await.unwrap();
    let addr = rx.http_addr;

    // Discovery documents.
    let desc = http(addr, "GET", "/desc.xml", &[], "").await;
    assert_eq!(desc.status, 200);
    assert!(desc.body.contains("<friendlyName>Test receiver</friendlyName>"));
    assert!(http(addr, "GET", "/scpd/avt.xml", &[], "").await.body.contains("<name>SetAVTransportURI</name>"));
    assert_eq!(http(addr, "GET", "/nope", &[], "").await.status, 404);

    // Subscribe to AVTransport events; the first event carries the full state.
    let events = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let callback = format!("<http://{}/cb>", events.local_addr().unwrap());
    let sub = http(
        addr,
        "SUBSCRIBE",
        "/evt/avt",
        &[("CALLBACK", &callback), ("NT", "upnp:event"), ("TIMEOUT", "Second-300")],
        "",
    )
    .await;
    assert_eq!(sub.status, 200);
    let sid =
        sub.head.lines().find_map(|l| l.strip_prefix("sid: ").or_else(|| l.strip_prefix("SID: "))).unwrap().to_string();
    let first = next_event(&events).await;
    assert!(first.contains("SEQ: 0") && first.contains("NO_MEDIA_PRESENT"), "{first}");

    // Cast: the player is told to load, subscribers see TRANSITIONING.
    let set = avt(
        addr,
        "SetAVTransportURI",
        "<CurrentURI>http://10.0.0.9/e1.mp4</CurrentURI><CurrentURIMetaData></CurrentURIMetaData>",
    )
    .await;
    assert_eq!(set.status, 200, "{}", set.body);
    assert!(set.body.contains("SetAVTransportURIResponse"));
    match next_command(&mut rx.commands).await {
        PlayerCommand::Load { gen, media } => {
            assert_eq!(gen, Gen(1));
            assert_eq!(media.url, "http://10.0.0.9/e1.mp4");
            assert_eq!(media.mime.as_deref(), Some("video/mp4"));
        }
        other => panic!("expected Load, got {other:?}"),
    }
    assert!(next_event(&events).await.contains("TRANSITIONING"));

    // Play while loading is remembered (not error 701) and starts once loaded.
    assert_eq!(avt(addr, "Play", "<Speed>1</Speed>").await.status, 200);
    rx.handle.player_event(PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(1500)) });
    assert_eq!(next_command(&mut rx.commands).await, PlayerCommand::Play);
    assert!(next_event(&events).await.contains("PLAYING"));
    let info = avt(addr, "GetTransportInfo", "").await;
    assert!(info.body.contains("<CurrentTransportState>PLAYING</CurrentTransportState>"), "{}", info.body);
    let pos = avt(addr, "GetPositionInfo", "").await;
    assert!(pos.body.contains("<TrackDuration>0:25:00</TrackDuration>"), "{}", pos.body);

    // Errors come back as UPnP faults with the right code.
    let bad = avt(addr, "Seek", "<Unit>TRACK_NR</Unit><Target>2</Target>").await;
    assert_eq!(bad.status, 500);
    assert!(bad.body.contains("<errorCode>710</errorCode>"));

    // Unsubscribe once; a second time is a precondition failure.
    assert_eq!(http(addr, "UNSUBSCRIBE", "/evt/avt", &[("SID", &sid)], "").await.status, 200);
    assert_eq!(http(addr, "UNSUBSCRIBE", "/evt/avt", &[("SID", &sid)], "").await.status, 412);
    rx.handle.shutdown().await;
}

#[tokio::test]
async fn bad_subscriptions_are_refused() {
    let rx = castable_net::start(local_config()).await.unwrap();
    let r = http(rx.http_addr, "SUBSCRIBE", "/evt/avt", &[("CALLBACK", "<http://127.0.0.1:1/>"), ("NT", "wrong")], "")
        .await;
    assert_eq!(r.status, 412);
    let r = http(rx.http_addr, "SUBSCRIBE", "/evt/avt", &[("SID", "uuid:unknown")], "").await;
    assert_eq!(r.status, 412);
}

#[tokio::test]
async fn answers_m_search() {
    let port = {
        let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let rx = castable_net::start(Config { ssdp_port: Some(port), ..local_config() }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let search = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    client.send_to(search.as_bytes(), ("127.0.0.1", port)).await.unwrap();
    let mut buf = [0u8; 2048];
    let (n, _) = tokio::time::timeout(Duration::from_secs(3), client.recv_from(&mut buf))
        .await
        .expect("no SSDP answer")
        .unwrap();
    let answer = String::from_utf8_lossy(&buf[..n]);
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(answer.contains(&format!(":{}/desc.xml", rx.http_addr.port())), "{answer}");
    assert!(answer.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1"));
    rx.handle.shutdown().await;
}
