//! The HTTP server: device and service descriptions, SOAP control, and GENA
//! subscriptions. Routing is generated from `castable_core::Service`.

use crate::actor::Msg;
use crate::gena::SubscribeRequest;
use bytes::Bytes;
use castable_core::device::DESCRIPTION_PATH;
use castable_core::{soap, Action, DeviceInfo, ErrorCode, Service, UpnpError};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CONTENT_TYPE, SERVER};
use hyper::{Method, Request, Response, StatusCode};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

const MAX_BODY: usize = 256 * 1024;

pub struct Ctx {
    /// The device as it is now: `Handle::rename` changes it.
    pub device: tokio::sync::watch::Receiver<DeviceInfo>,
    pub actor: mpsc::UnboundedSender<Msg>,
}

type Resp = Response<Full<Bytes>>;

/// What a request is for, worked out once from method and path.
enum Route {
    Description,
    Scpd(Service),
    Control(Service),
    Subscribe(Service),
    Unsubscribe,
    NotFound,
}

fn route(method: &Method, path: &str) -> Route {
    if method == Method::GET && path == DESCRIPTION_PATH {
        return Route::Description;
    }
    let Some(service) = Service::from_path(path) else { return Route::NotFound };
    match method.as_str() {
        "GET" if path == service.scpd_path() => Route::Scpd(service),
        "POST" if path == service.control_path() => Route::Control(service),
        "SUBSCRIBE" if path == service.event_path() => Route::Subscribe(service),
        "UNSUBSCRIBE" if path == service.event_path() => Route::Unsubscribe,
        _ => Route::NotFound,
    }
}

pub async fn handle(
    req: Request<Incoming>,
    ctx: Arc<Ctx>,
    peer: std::net::SocketAddr,
) -> Result<Resp, std::convert::Infallible> {
    let started = std::time::Instant::now();
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let resp = match route(&method, &path) {
        Route::Description => xml(StatusCode::OK, ctx.device.borrow().description_xml()),
        Route::Scpd(service) => xml(StatusCode::OK, service.scpd()),
        Route::Control(service) => control(service, req, &ctx, peer).await,
        Route::Subscribe(service) => subscribe(service, &req, &ctx).await,
        Route::Unsubscribe => unsubscribe(&req, &ctx).await,
        Route::NotFound => empty(StatusCode::NOT_FOUND),
    };
    tracing::debug!(
        peer = %peer.ip(),
        method = %method,
        path = %path,
        status = resp.status().as_u16(),
        elapsed_us = started.elapsed().as_micros() as u64,
        "http"
    );
    Ok(with_server(resp, &ctx.device.borrow()))
}

async fn control(service: Service, req: Request<Incoming>, ctx: &Ctx, peer: std::net::SocketAddr) -> Resp {
    // Which phone and app sent it: worth a line on every cast.
    let agent = header(&req, "user-agent").unwrap_or("").to_owned();
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => String::from_utf8_lossy(&b.to_bytes()).into_owned(),
        Err(_) => return empty(StatusCode::PAYLOAD_TOO_LARGE),
    };
    let result = async {
        let call = soap::parse_request(&body).inspect_err(|e| {
            let c = ErrorCode::from(e);
            tracing::info!(service = service.id(), code = %c.id(), error = c.name(), category = c.category().name(), summary = c.summary(), upnp = e.code(), bytes = body.len(), "control refused: the request isn't valid SOAP")
        })?;
        let action = Action::parse(service, &call).inspect_err(|e| {
            let c = ErrorCode::from(e);
            tracing::info!(service = service.id(), action = %call.action, code = %c.id(), error = c.name(), category = c.category().name(), summary = c.summary(), upnp = e.code(), "control refused")
        })?;
        if matches!(action, Action::SetUri { .. }) {
            tracing::info!(peer = %peer.ip(), user_agent = %agent, "cast from");
        }
        let (reply, rx) = oneshot::channel();
        ctx.actor.send(Msg::Act { action, reply }).map_err(|_| UpnpError::ActionFailed)?;
        let out = rx.await.map_err(|_| UpnpError::ActionFailed)??;
        Ok::<_, UpnpError>(soap::response(service, &call.action, &out))
    }
    .await;
    match result {
        Ok(xml_body) => with_ext(xml(StatusCode::OK, xml_body)),
        Err(e) => xml(StatusCode::INTERNAL_SERVER_ERROR, soap::fault(&e)),
    }
}

fn header<'a>(req: &'a Request<Incoming>, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

async fn subscribe(service: Service, req: &Request<Incoming>, ctx: &Ctx) -> Resp {
    let Some(sub) = SubscribeRequest::from_headers(
        header(req, "callback"),
        header(req, "nt"),
        header(req, "sid"),
        header(req, "timeout"),
    ) else {
        return empty(StatusCode::PRECONDITION_FAILED);
    };
    let (reply, rx) = oneshot::channel();
    if ctx.actor.send(Msg::Subscribe { service, req: sub, reply }).is_err() {
        return empty(StatusCode::INTERNAL_SERVER_ERROR);
    }
    match rx.await.ok().flatten() {
        Some((sid, timeout)) => {
            let mut r = empty(StatusCode::OK);
            let h = r.headers_mut();
            h.insert("SID", HeaderValue::from_str(&sid).expect("uuid is a valid header"));
            h.insert("TIMEOUT", HeaderValue::from_str(&format!("Second-{}", timeout.as_secs())).expect("valid"));
            r
        }
        None => empty(StatusCode::PRECONDITION_FAILED),
    }
}

async fn unsubscribe(req: &Request<Incoming>, ctx: &Ctx) -> Resp {
    let Some(sid) = header(req, "sid") else { return empty(StatusCode::PRECONDITION_FAILED) };
    let (reply, rx) = oneshot::channel();
    let _ = ctx.actor.send(Msg::Unsubscribe { sid: sid.trim().to_string(), reply });
    if rx.await.unwrap_or(false) {
        empty(StatusCode::OK)
    } else {
        empty(StatusCode::PRECONDITION_FAILED)
    }
}

fn xml(status: StatusCode, body: String) -> Resp {
    let mut r = Response::new(Full::new(Bytes::from(body)));
    *r.status_mut() = status;
    r.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("text/xml; charset=\"utf-8\""));
    r
}

fn empty(status: StatusCode) -> Resp {
    let mut r = Response::new(Full::new(Bytes::new()));
    *r.status_mut() = status;
    r
}

fn with_ext(mut r: Resp) -> Resp {
    r.headers_mut().insert("EXT", HeaderValue::from_static(""));
    r
}

fn with_server(mut r: Resp, device: &DeviceInfo) -> Resp {
    if let Ok(v) = HeaderValue::from_str(&device.server) {
        r.headers_mut().insert(SERVER, v);
    }
    r
}
