//! The MediaRenderer: typed UPnP actions on top of the session.
//!
//! Requests are parsed once into [`Action`], a discriminated union, so every
//! later step works with typed values instead of strings.

use crate::didl::descriptor_from;
use crate::error::UpnpError;
use crate::service::{sink_protocol_info, Service};
use crate::session::{Input, Phase, Playback, PlayerCommand, PlayerEvent, ScreenCommand, SenderCommand, Session};
use crate::soap::RawCall;
use crate::time::{format_hms, parse_hms};
use crate::xml::escape;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // AVTransport
    SetUri { uri: String, metadata: String },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    GetTransportInfo,
    GetPositionInfo,
    GetMediaInfo,
    GetTransportSettings,
    GetCurrentTransportActions,
    GetDeviceCapabilities,
    // RenderingControl
    GetVolume,
    SetVolume(u8),
    GetMute,
    SetMute(bool),
    // ConnectionManager
    GetProtocolInfo,
    GetCurrentConnectionIds,
    GetCurrentConnectionInfo,
}

impl Action {
    /// The UPnP action name, for logs (no allocation).
    pub fn name(&self) -> &'static str {
        match self {
            Action::SetUri { .. } => "SetAVTransportURI",
            Action::Play => "Play",
            Action::Pause => "Pause",
            Action::Stop => "Stop",
            Action::Seek(_) => "Seek",
            Action::GetTransportInfo => "GetTransportInfo",
            Action::GetPositionInfo => "GetPositionInfo",
            Action::GetMediaInfo => "GetMediaInfo",
            Action::GetTransportSettings => "GetTransportSettings",
            Action::GetCurrentTransportActions => "GetCurrentTransportActions",
            Action::GetDeviceCapabilities => "GetDeviceCapabilities",
            Action::GetVolume => "GetVolume",
            Action::SetVolume(_) => "SetVolume",
            Action::GetMute => "GetMute",
            Action::SetMute(_) => "SetMute",
            Action::GetProtocolInfo => "GetProtocolInfo",
            Action::GetCurrentConnectionIds => "GetCurrentConnectionIDs",
            Action::GetCurrentConnectionInfo => "GetCurrentConnectionInfo",
        }
    }

    /// Actions that only read state: logged at debug, since senders poll them.
    pub fn is_query(&self) -> bool {
        self.name().starts_with("Get")
    }

    /// Types a raw SOAP call for `service`. Unknown actions and bad
    /// arguments become the matching UPnP error.
    pub fn parse(service: Service, call: &RawCall) -> Result<Action, UpnpError> {
        use Action as A;
        if service != Service::ConnectionManager && call.arg("InstanceID").is_some_and(|id| id.trim() != "0") {
            return Err(UpnpError::InvalidInstanceId);
        }
        let action = match (service, call.action.as_str()) {
            (Service::AvTransport, "SetAVTransportURI") => A::SetUri {
                uri: call.require("CurrentURI")?.trim().to_string(),
                metadata: call.arg("CurrentURIMetaData").unwrap_or_default().to_string(),
            },
            (Service::AvTransport, "Play") => A::Play,
            (Service::AvTransport, "Pause") => A::Pause,
            (Service::AvTransport, "Stop") => A::Stop,
            (Service::AvTransport, "Seek") => match call.require("Unit")? {
                "REL_TIME" | "ABS_TIME" => {
                    A::Seek(parse_hms(call.require("Target")?).ok_or(UpnpError::IllegalSeekTarget)?)
                }
                _ => return Err(UpnpError::SeekModeNotSupported),
            },
            (Service::AvTransport, "GetTransportInfo") => A::GetTransportInfo,
            (Service::AvTransport, "GetPositionInfo") => A::GetPositionInfo,
            (Service::AvTransport, "GetMediaInfo") => A::GetMediaInfo,
            (Service::AvTransport, "GetTransportSettings") => A::GetTransportSettings,
            (Service::AvTransport, "GetCurrentTransportActions") => A::GetCurrentTransportActions,
            (Service::AvTransport, "GetDeviceCapabilities") => A::GetDeviceCapabilities,
            (Service::RenderingControl, "GetVolume") => A::GetVolume,
            (Service::RenderingControl, "SetVolume") => {
                let v: u32 = call.require("DesiredVolume")?.trim().parse().map_err(|_| UpnpError::InvalidArgs)?;
                A::SetVolume(v.min(100) as u8)
            }
            (Service::RenderingControl, "GetMute") => A::GetMute,
            (Service::RenderingControl, "SetMute") => A::SetMute(match call.require("DesiredMute")?.trim() {
                "1" | "true" | "True" | "yes" => true,
                "0" | "false" | "False" | "no" => false,
                _ => return Err(UpnpError::InvalidArgs),
            }),
            (Service::ConnectionManager, "GetProtocolInfo") => A::GetProtocolInfo,
            (Service::ConnectionManager, "GetCurrentConnectionIDs") => A::GetCurrentConnectionIds,
            (Service::ConnectionManager, "GetCurrentConnectionInfo") => A::GetCurrentConnectionInfo,
            _ => return Err(UpnpError::InvalidAction),
        };
        Ok(action)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    /// Output arguments of the SOAP response, in SCPD order.
    pub out: Vec<(&'static str, String)>,
    pub commands: Vec<PlayerCommand>,
    /// Services whose evented state changed (send a LastChange to subscribers).
    pub notify: Vec<Service>,
}

/// The current track's fields as UPnP writes them.
struct Track {
    uri: String,
    duration: String,
    position: String,
    count: &'static str,
}

#[derive(Debug, Clone)]
pub struct Renderer {
    session: Session,
    /// The raw DIDL-Lite of the current cast, echoed back to senders.
    metadata: String,
    volume: u8,
    muted: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self { session: Session::new(), metadata: String::new(), volume: 50, muted: false }
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn volume(&self) -> (u8, bool) {
        (self.volume, self.muted)
    }

    /// Runs one control action.
    pub fn act(&mut self, action: Action) -> Result<Reply, UpnpError> {
        use Action as A;
        let sender = |r: &mut Self, c: SenderCommand| r.drive(Input::Sender(c));
        Ok(match action {
            A::SetUri { uri, metadata } => {
                if uri.is_empty() {
                    return Err(UpnpError::InvalidArgs);
                }
                let media = descriptor_from(&uri, Some(&metadata));
                let reply = sender(self, SenderCommand::SetUri(media))?;
                self.metadata = metadata;
                reply
            }
            A::Play => sender(self, SenderCommand::Play)?,
            A::Pause => sender(self, SenderCommand::Pause)?,
            A::Stop => sender(self, SenderCommand::Stop)?,
            A::Seek(to) => sender(self, SenderCommand::Seek { to })?,
            A::SetVolume(volume) => {
                self.mixer(PlayerCommand::SetVolume { volume }, volume != self.volume, |r| r.volume = volume)
            }
            A::SetMute(muted) => {
                self.mixer(PlayerCommand::SetMuted { muted }, muted != self.muted, |r| r.muted = muted)
            }
            query => Reply { out: self.query(&query), ..Reply::default() },
        })
    }

    /// Feeds a player event (load finished, progress, ended...).
    /// A press on the receiver's own screen. Dropped (no change, no error)
    /// if the cast it was made on has been replaced.
    pub fn on_screen(&mut self, press: ScreenCommand) -> Result<Reply, UpnpError> {
        self.drive(Input::Screen(press))
    }

    pub fn on_player(&mut self, event: PlayerEvent) -> Reply {
        self.drive(Input::Player(event)).unwrap_or_default()
    }

    fn drive(&mut self, input: Input) -> Result<Reply, UpnpError> {
        let outcome = self.session.handle(input)?;
        Ok(Reply {
            out: vec![],
            commands: outcome.commands,
            notify: if outcome.changed { vec![Service::AvTransport] } else { vec![] },
        })
    }

    fn mixer(&mut self, cmd: PlayerCommand, changed: bool, apply: impl FnOnce(&mut Self)) -> Reply {
        apply(self);
        Reply {
            out: vec![],
            commands: vec![cmd],
            notify: if changed { vec![Service::RenderingControl] } else { vec![] },
        }
    }

    /// The current track as UPnP strings, shared by queries and events.
    fn track(&self) -> Track {
        let cast = self.session.cast();
        let hms = |d: Option<std::time::Duration>| d.map_or_else(|| "0:00:00".to_string(), format_hms);
        Track {
            uri: cast.map(|c| c.media.url.clone()).unwrap_or_default(),
            duration: hms(cast.and_then(|c| c.duration)),
            position: hms(cast.map(|c| c.position)),
            count: if cast.is_some() { "1" } else { "0" },
        }
    }

    fn query(&self, action: &Action) -> Vec<(&'static str, String)> {
        use Action as A;
        let s = &self.session;
        let Track { uri, duration, position, count } = self.track();
        let o = |pairs: &[(&'static str, &str)]| pairs.iter().map(|(k, v)| (*k, v.to_string())).collect();
        match action {
            A::GetTransportInfo => o(&[
                ("CurrentTransportState", s.transport_state().as_str()),
                ("CurrentTransportStatus", s.transport_status()),
                ("CurrentSpeed", "1"),
            ]),
            A::GetPositionInfo => o(&[
                ("Track", count),
                ("TrackDuration", &duration),
                ("TrackMetaData", &self.metadata),
                ("TrackURI", &uri),
                ("RelTime", &position),
                ("AbsTime", &position),
                ("RelCount", "2147483647"),
                ("AbsCount", "2147483647"),
            ]),
            A::GetMediaInfo => o(&[
                ("NrTracks", count),
                ("MediaDuration", &duration),
                ("CurrentURI", &uri),
                ("CurrentURIMetaData", &self.metadata),
                ("NextURI", ""),
                ("NextURIMetaData", ""),
                ("PlayMedium", "NETWORK"),
                ("RecordMedium", "NOT_IMPLEMENTED"),
                ("WriteStatus", "NOT_IMPLEMENTED"),
            ]),
            A::GetTransportSettings => o(&[("PlayMode", "NORMAL"), ("RecQualityMode", "NOT_IMPLEMENTED")]),
            A::GetCurrentTransportActions => o(&[("Actions", &self.available_actions())]),
            A::GetDeviceCapabilities => {
                o(&[("PlayMedia", "NETWORK"), ("RecMedia", "NOT_IMPLEMENTED"), ("RecQualityModes", "NOT_IMPLEMENTED")])
            }
            A::GetVolume => o(&[("CurrentVolume", &self.volume.to_string())]),
            A::GetMute => o(&[("CurrentMute", if self.muted { "1" } else { "0" })]),
            A::GetProtocolInfo => o(&[("Source", ""), ("Sink", &sink_protocol_info())]),
            A::GetCurrentConnectionIds => o(&[("ConnectionIDs", "0")]),
            A::GetCurrentConnectionInfo => o(&[
                ("RcsID", "0"),
                ("AVTransportID", "0"),
                ("ProtocolInfo", ""),
                ("PeerConnectionManager", ""),
                ("PeerConnectionID", "-1"),
                ("Direction", "Input"),
                ("Status", "OK"),
            ]),
            _ => vec![],
        }
    }

    /// The transport actions that would succeed now, derived from the phase.
    fn available_actions(&self) -> String {
        let actions: &[&str] = match self.session.phase() {
            Phase::Idle => &[],
            Phase::Loading { .. } => &["Play", "Pause", "Stop", "Seek"],
            Phase::Loaded { playback: Playback::Playing, .. } => &["Pause", "Stop", "Seek"],
            Phase::Loaded { .. } => &["Play", "Stop", "Seek"],
            Phase::Unloaded { .. } => &["Play", "Stop"],
        };
        actions.join(",")
    }

    /// The LastChange event body for `service`, ready to put in a GENA NOTIFY.
    pub fn last_change(&self, service: Service) -> String {
        let vars: Vec<(&str, String)> = match service {
            Service::AvTransport => {
                let s = &self.session;
                let Track { uri, duration, .. } = self.track();
                vec![
                    ("TransportState", s.transport_state().as_str().into()),
                    ("TransportStatus", s.transport_status().into()),
                    ("CurrentTransportActions", self.available_actions()),
                    ("AVTransportURI", uri.clone()),
                    ("CurrentTrackURI", uri),
                    ("CurrentTrackDuration", duration),
                ]
            }
            Service::RenderingControl => {
                vec![("Volume", self.volume.to_string()), ("Mute", u8::from(self.muted).to_string())]
            }
            Service::ConnectionManager => vec![],
        };
        let ns = match service {
            Service::RenderingControl => "urn:schemas-upnp-org:metadata-1-0/RCS/",
            _ => "urn:schemas-upnp-org:metadata-1-0/AVT/",
        };
        let channel = if service == Service::RenderingControl { r#" channel="Master""# } else { "" };
        let body: String = vars.iter().map(|(k, v)| format!(r#"<{k}{channel} val="{}"/>"#, escape(v))).collect();
        let event = format!(r#"<Event xmlns="{ns}"><InstanceID val="0">{body}</InstanceID></Event>"#);
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>{}</LastChange></e:property></e:propertyset>"#,
            escape(&event)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Gen;
    use crate::soap::parse_request;

    fn call(service: Service, body: &str) -> Action {
        let xml = format!(
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>{body}</s:Body></s:Envelope>"#
        );
        Action::parse(service, &parse_request(&xml).unwrap()).unwrap()
    }

    #[test]
    fn a_full_cast() {
        let mut r = Renderer::new();
        let set = call(Service::AvTransport, "<u:SetAVTransportURI><InstanceID>0</InstanceID><CurrentURI>http://x/a.mp4</CurrentURI><CurrentURIMetaData></CurrentURIMetaData></u:SetAVTransportURI>");
        let reply = r.act(set).unwrap();
        assert!(matches!(reply.commands[0], PlayerCommand::Load { gen: Gen(1), .. }));
        assert_eq!(reply.notify, vec![Service::AvTransport]);
        r.act(Action::Play).unwrap();
        let reply = r.on_player(PlayerEvent::Loaded { gen: Gen(1), duration: Some(Duration::from_secs(90)) });
        assert_eq!(reply.commands, vec![PlayerCommand::Play]);
        let info = r.act(Action::GetTransportInfo).unwrap().out;
        assert_eq!(info[0], ("CurrentTransportState", "PLAYING".to_string()));
        let pos = r.act(Action::GetPositionInfo).unwrap().out;
        assert_eq!(pos[1], ("TrackDuration", "0:01:30".to_string()));
        assert!(r.last_change(Service::AvTransport).contains("TransportState val=&quot;PLAYING&quot;"));
    }

    #[test]
    fn typed_parsing_rejects_bad_calls() {
        let bad = |svc, body: &str| {
            let xml = format!("<Envelope><Body>{body}</Body></Envelope>");
            Action::parse(svc, &parse_request(&xml).unwrap()).unwrap_err()
        };
        assert_eq!(
            bad(Service::AvTransport, "<Seek><InstanceID>0</InstanceID><Unit>TRACK_NR</Unit><Target>2</Target></Seek>"),
            UpnpError::SeekModeNotSupported
        );
        assert_eq!(bad(Service::AvTransport, "<Play><InstanceID>3</InstanceID></Play>"), UpnpError::InvalidInstanceId);
        assert_eq!(bad(Service::AvTransport, "<Record><InstanceID>0</InstanceID></Record>"), UpnpError::InvalidAction);
        assert_eq!(
            bad(
                Service::RenderingControl,
                "<SetMute><InstanceID>0</InstanceID><DesiredMute>maybe</DesiredMute></SetMute>"
            ),
            UpnpError::InvalidArgs
        );
        assert_eq!(
            call(Service::AvTransport, "<Seek><Unit>REL_TIME</Unit><Target>0:00:30</Target></Seek>"),
            Action::Seek(Duration::from_secs(30))
        );
    }

    /// The service table (what senders are told) and `Action` (what is
    /// parsed and logged) must name the same actions.
    #[test]
    fn every_advertised_action_parses_to_itself() {
        use crate::service::{ActionSpec, ArgSpec, Dir};
        let value = |arg: &str| match arg {
            "CurrentURI" => "http://x/a.mp4",
            "Unit" => "REL_TIME",
            "Target" => "0:00:01",
            "DesiredMute" => "1",
            "Speed" => "1",
            _ => "0",
        };
        let mut count = 0;
        for service in Service::ALL {
            for ActionSpec(name, args) in service.actions() {
                let call = crate::soap::RawCall {
                    action: name.to_string(),
                    args: args
                        .iter()
                        .filter(|ArgSpec(_, dir, _)| *dir == Dir::In)
                        .map(|ArgSpec(a, _, _)| (a.to_string(), value(a).to_string()))
                        .collect(),
                };
                let action = Action::parse(service, &call).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(action.name(), *name);
                count += 1;
            }
        }
        assert_eq!(count, 18, "every Action variant is advertised exactly once");
    }

    #[test]
    fn volume_events_only_when_changed() {
        let mut r = Renderer::new();
        assert_eq!(r.act(Action::SetVolume(70)).unwrap().notify, vec![Service::RenderingControl]);
        assert!(r.act(Action::SetVolume(70)).unwrap().notify.is_empty());
        assert_eq!(r.act(Action::GetVolume).unwrap().out, vec![("CurrentVolume", "70".to_string())]);
    }
}
