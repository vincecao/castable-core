//! The three UPnP services, their URLs and their SCPDs, defined once and used
//! by the device description, SSDP, HTTP routing and SOAP responses.

use crate::xml::escape;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    AvTransport,
    RenderingControl,
    ConnectionManager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
}

pub struct ArgSpec(pub &'static str, pub Dir, pub &'static str);
pub struct ActionSpec(pub &'static str, pub &'static [ArgSpec]);
/// name, dataType, sendEvents, allowed values
pub struct VarSpec(pub &'static str, pub &'static str, pub bool, pub &'static [&'static str]);

use Dir::{In, Out};

const INSTANCE: ArgSpec = ArgSpec("InstanceID", In, "A_ARG_TYPE_InstanceID");

const AVT_ACTIONS: &[ActionSpec] = &[
    ActionSpec(
        "SetAVTransportURI",
        &[
            INSTANCE,
            ArgSpec("CurrentURI", In, "AVTransportURI"),
            ArgSpec("CurrentURIMetaData", In, "AVTransportURIMetaData"),
        ],
    ),
    ActionSpec("Play", &[INSTANCE, ArgSpec("Speed", In, "TransportPlaySpeed")]),
    ActionSpec("Pause", &[INSTANCE]),
    ActionSpec("Stop", &[INSTANCE]),
    ActionSpec(
        "Seek",
        &[INSTANCE, ArgSpec("Unit", In, "A_ARG_TYPE_SeekMode"), ArgSpec("Target", In, "A_ARG_TYPE_SeekTarget")],
    ),
    ActionSpec(
        "GetTransportInfo",
        &[
            INSTANCE,
            ArgSpec("CurrentTransportState", Out, "TransportState"),
            ArgSpec("CurrentTransportStatus", Out, "TransportStatus"),
            ArgSpec("CurrentSpeed", Out, "TransportPlaySpeed"),
        ],
    ),
    ActionSpec(
        "GetPositionInfo",
        &[
            INSTANCE,
            ArgSpec("Track", Out, "CurrentTrack"),
            ArgSpec("TrackDuration", Out, "CurrentTrackDuration"),
            ArgSpec("TrackMetaData", Out, "CurrentTrackMetaData"),
            ArgSpec("TrackURI", Out, "CurrentTrackURI"),
            ArgSpec("RelTime", Out, "RelativeTimePosition"),
            ArgSpec("AbsTime", Out, "AbsoluteTimePosition"),
            ArgSpec("RelCount", Out, "RelativeCounterPosition"),
            ArgSpec("AbsCount", Out, "AbsoluteCounterPosition"),
        ],
    ),
    ActionSpec(
        "GetMediaInfo",
        &[
            INSTANCE,
            ArgSpec("NrTracks", Out, "NumberOfTracks"),
            ArgSpec("MediaDuration", Out, "CurrentMediaDuration"),
            ArgSpec("CurrentURI", Out, "AVTransportURI"),
            ArgSpec("CurrentURIMetaData", Out, "AVTransportURIMetaData"),
            ArgSpec("NextURI", Out, "NextAVTransportURI"),
            ArgSpec("NextURIMetaData", Out, "NextAVTransportURIMetaData"),
            ArgSpec("PlayMedium", Out, "PlaybackStorageMedium"),
            ArgSpec("RecordMedium", Out, "RecordStorageMedium"),
            ArgSpec("WriteStatus", Out, "RecordMediumWriteStatus"),
        ],
    ),
    ActionSpec(
        "GetTransportSettings",
        &[
            INSTANCE,
            ArgSpec("PlayMode", Out, "CurrentPlayMode"),
            ArgSpec("RecQualityMode", Out, "CurrentRecordQualityMode"),
        ],
    ),
    ActionSpec("GetCurrentTransportActions", &[INSTANCE, ArgSpec("Actions", Out, "CurrentTransportActions")]),
    ActionSpec(
        "GetDeviceCapabilities",
        &[
            INSTANCE,
            ArgSpec("PlayMedia", Out, "PossiblePlaybackStorageMedia"),
            ArgSpec("RecMedia", Out, "PossibleRecordStorageMedia"),
            ArgSpec("RecQualityModes", Out, "PossibleRecordQualityModes"),
        ],
    ),
];

const AVT_VARS: &[VarSpec] = &[
    VarSpec(
        "TransportState",
        "string",
        false,
        &["STOPPED", "PLAYING", "TRANSITIONING", "PAUSED_PLAYBACK", "NO_MEDIA_PRESENT"],
    ),
    VarSpec("TransportStatus", "string", false, &["OK", "ERROR_OCCURRED"]),
    VarSpec("TransportPlaySpeed", "string", false, &["1"]),
    VarSpec("PlaybackStorageMedium", "string", false, &["NETWORK"]),
    VarSpec("RecordStorageMedium", "string", false, &["NOT_IMPLEMENTED"]),
    VarSpec("PossiblePlaybackStorageMedia", "string", false, &[]),
    VarSpec("PossibleRecordStorageMedia", "string", false, &[]),
    VarSpec("PossibleRecordQualityModes", "string", false, &[]),
    VarSpec("CurrentPlayMode", "string", false, &["NORMAL"]),
    VarSpec("CurrentRecordQualityMode", "string", false, &["NOT_IMPLEMENTED"]),
    VarSpec("RecordMediumWriteStatus", "string", false, &["NOT_IMPLEMENTED"]),
    VarSpec("NumberOfTracks", "ui4", false, &[]),
    VarSpec("CurrentTrack", "ui4", false, &[]),
    VarSpec("CurrentTrackDuration", "string", false, &[]),
    VarSpec("CurrentMediaDuration", "string", false, &[]),
    VarSpec("CurrentTrackMetaData", "string", false, &[]),
    VarSpec("CurrentTrackURI", "string", false, &[]),
    VarSpec("AVTransportURI", "string", false, &[]),
    VarSpec("AVTransportURIMetaData", "string", false, &[]),
    VarSpec("NextAVTransportURI", "string", false, &[]),
    VarSpec("NextAVTransportURIMetaData", "string", false, &[]),
    VarSpec("RelativeTimePosition", "string", false, &[]),
    VarSpec("AbsoluteTimePosition", "string", false, &[]),
    VarSpec("RelativeCounterPosition", "i4", false, &[]),
    VarSpec("AbsoluteCounterPosition", "i4", false, &[]),
    VarSpec("CurrentTransportActions", "string", false, &[]),
    VarSpec("LastChange", "string", true, &[]),
    VarSpec("A_ARG_TYPE_SeekMode", "string", false, &["REL_TIME", "ABS_TIME"]),
    VarSpec("A_ARG_TYPE_SeekTarget", "string", false, &[]),
    VarSpec("A_ARG_TYPE_InstanceID", "ui4", false, &[]),
];

const RC_ACTIONS: &[ActionSpec] = &[
    ActionSpec(
        "GetVolume",
        &[INSTANCE, ArgSpec("Channel", In, "A_ARG_TYPE_Channel"), ArgSpec("CurrentVolume", Out, "Volume")],
    ),
    ActionSpec(
        "SetVolume",
        &[INSTANCE, ArgSpec("Channel", In, "A_ARG_TYPE_Channel"), ArgSpec("DesiredVolume", In, "Volume")],
    ),
    ActionSpec(
        "GetMute",
        &[INSTANCE, ArgSpec("Channel", In, "A_ARG_TYPE_Channel"), ArgSpec("CurrentMute", Out, "Mute")],
    ),
    ActionSpec(
        "SetMute",
        &[INSTANCE, ArgSpec("Channel", In, "A_ARG_TYPE_Channel"), ArgSpec("DesiredMute", In, "Mute")],
    ),
];

const RC_VARS: &[VarSpec] = &[
    VarSpec("Volume", "ui2", false, &[]),
    VarSpec("Mute", "boolean", false, &[]),
    VarSpec("LastChange", "string", true, &[]),
    VarSpec("A_ARG_TYPE_Channel", "string", false, &["Master"]),
    VarSpec("A_ARG_TYPE_InstanceID", "ui4", false, &[]),
];

const CM_ACTIONS: &[ActionSpec] = &[
    ActionSpec(
        "GetProtocolInfo",
        &[ArgSpec("Source", Out, "SourceProtocolInfo"), ArgSpec("Sink", Out, "SinkProtocolInfo")],
    ),
    ActionSpec("GetCurrentConnectionIDs", &[ArgSpec("ConnectionIDs", Out, "CurrentConnectionIDs")]),
    ActionSpec(
        "GetCurrentConnectionInfo",
        &[
            ArgSpec("ConnectionID", In, "A_ARG_TYPE_ConnectionID"),
            ArgSpec("RcsID", Out, "A_ARG_TYPE_RcsID"),
            ArgSpec("AVTransportID", Out, "A_ARG_TYPE_AVTransportID"),
            ArgSpec("ProtocolInfo", Out, "A_ARG_TYPE_ProtocolInfo"),
            ArgSpec("PeerConnectionManager", Out, "A_ARG_TYPE_ConnectionManager"),
            ArgSpec("PeerConnectionID", Out, "A_ARG_TYPE_ConnectionID"),
            ArgSpec("Direction", Out, "A_ARG_TYPE_Direction"),
            ArgSpec("Status", Out, "A_ARG_TYPE_ConnectionStatus"),
        ],
    ),
];

const CM_VARS: &[VarSpec] = &[
    VarSpec("SourceProtocolInfo", "string", true, &[]),
    VarSpec("SinkProtocolInfo", "string", true, &[]),
    VarSpec("CurrentConnectionIDs", "string", true, &[]),
    VarSpec(
        "A_ARG_TYPE_ConnectionStatus",
        "string",
        false,
        &["OK", "ContentFormatMismatch", "InsufficientBandwidth", "UnreliableChannel", "Unknown"],
    ),
    VarSpec("A_ARG_TYPE_ConnectionManager", "string", false, &[]),
    VarSpec("A_ARG_TYPE_Direction", "string", false, &["Input", "Output"]),
    VarSpec("A_ARG_TYPE_ProtocolInfo", "string", false, &[]),
    VarSpec("A_ARG_TYPE_ConnectionID", "i4", false, &[]),
    VarSpec("A_ARG_TYPE_AVTransportID", "i4", false, &[]),
    VarSpec("A_ARG_TYPE_RcsID", "i4", false, &[]),
];

/// MIME types the receiver accepts, advertised in `GetProtocolInfo`.
pub const SINK_MIME_TYPES: &[&str] = &[
    "video/mp4",
    "video/quicktime",
    "video/x-matroska",
    "video/mpeg",
    "application/vnd.apple.mpegurl",
    "application/x-mpegURL",
    "audio/mpeg",
    "audio/mp4",
    "audio/aac",
    "audio/flac",
    "image/jpeg",
    "image/png",
];

pub fn sink_protocol_info() -> String {
    SINK_MIME_TYPES.iter().map(|m| format!("http-get:*:{m}:*")).collect::<Vec<_>>().join(",")
}

impl Service {
    pub const ALL: [Service; 3] = [Service::AvTransport, Service::RenderingControl, Service::ConnectionManager];

    /// Short name, used in URLs.
    pub fn slug(self) -> &'static str {
        match self {
            Service::AvTransport => "avt",
            Service::RenderingControl => "rc",
            Service::ConnectionManager => "cm",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Service::AvTransport => "AVTransport",
            Service::RenderingControl => "RenderingControl",
            Service::ConnectionManager => "ConnectionManager",
        }
    }

    pub fn urn(self) -> String {
        format!("urn:schemas-upnp-org:service:{}:1", self.id())
    }

    pub fn scpd_path(self) -> String {
        format!("/scpd/{}.xml", self.slug())
    }
    pub fn control_path(self) -> String {
        format!("/ctl/{}", self.slug())
    }
    pub fn event_path(self) -> String {
        format!("/evt/{}", self.slug())
    }

    /// Finds the service whose control or event URL is `path`.
    pub fn from_path(path: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|s| path == s.control_path() || path == s.event_path() || path == s.scpd_path())
    }

    /// Finds the service from a SOAPACTION header value or service type URN.
    pub fn from_urn(urn: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|s| urn.contains(&format!(":service:{}:", s.id())))
    }

    pub fn actions(self) -> &'static [ActionSpec] {
        match self {
            Service::AvTransport => AVT_ACTIONS,
            Service::RenderingControl => RC_ACTIONS,
            Service::ConnectionManager => CM_ACTIONS,
        }
    }

    fn vars(self) -> &'static [VarSpec] {
        match self {
            Service::AvTransport => AVT_VARS,
            Service::RenderingControl => RC_VARS,
            Service::ConnectionManager => CM_VARS,
        }
    }

    /// The service description document (SCPD), generated from the tables above.
    pub fn scpd(self) -> String {
        let mut x = String::from(
            r#"<?xml version="1.0" encoding="utf-8"?><scpd xmlns="urn:schemas-upnp-org:service-1-0"><specVersion><major>1</major><minor>0</minor></specVersion><actionList>"#,
        );
        for ActionSpec(name, args) in self.actions() {
            x += &format!("<action><name>{name}</name><argumentList>");
            for ArgSpec(arg, dir, var) in *args {
                let dir = if *dir == In { "in" } else { "out" };
                x += &format!("<argument><name>{arg}</name><direction>{dir}</direction><relatedStateVariable>{var}</relatedStateVariable></argument>");
            }
            x += "</argumentList></action>";
        }
        x += "</actionList><serviceStateTable>";
        for VarSpec(name, ty, events, allowed) in self.vars() {
            let ev = if *events { "yes" } else { "no" };
            x += &format!(r#"<stateVariable sendEvents="{ev}"><name>{name}</name><dataType>{ty}</dataType>"#);
            if !allowed.is_empty() {
                x += "<allowedValueList>";
                for v in *allowed {
                    x += &format!("<allowedValue>{}</allowedValue>", escape(v));
                }
                x += "</allowedValueList>";
            }
            if *name == "Volume" {
                x += "<allowedValueRange><minimum>0</minimum><maximum>100</maximum><step>1</step></allowedValueRange>";
            }
            x += "</stateVariable>";
        }
        x += "</serviceStateTable></scpd>";
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_related_variable_exists() {
        for s in Service::ALL {
            for ActionSpec(action, args) in s.actions() {
                for ArgSpec(arg, _, var) in *args {
                    assert!(s.vars().iter().any(|v| v.0 == *var), "{action}.{arg} refers to missing {var}");
                }
            }
            assert!(crate::xml::parse(&s.scpd()).is_some(), "SCPD of {s:?} must be valid XML");
        }
    }

    #[test]
    fn routes_round_trip() {
        for s in Service::ALL {
            assert_eq!(Service::from_path(&s.control_path()), Some(s));
            assert_eq!(Service::from_urn(&s.urn()), Some(s));
        }
    }
}
