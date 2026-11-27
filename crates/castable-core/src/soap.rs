//! SOAP envelopes for UPnP control: parse a request, build a response or fault.

use crate::error::UpnpError;
use crate::service::Service;
use crate::xml::{escape, parse};

/// A control request before it is typed into an [`crate::renderer::Action`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCall {
    pub action: String,
    pub args: Vec<(String, String)>,
}

impl RawCall {
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn require(&self, name: &str) -> Result<&str, UpnpError> {
        self.arg(name).ok_or(UpnpError::InvalidArgs)
    }
}

/// Parses a SOAP request body. Tolerant of namespace prefixes and whitespace.
pub fn parse_request(body: &str) -> Result<RawCall, UpnpError> {
    let root = parse(body).ok_or(UpnpError::InvalidAction)?;
    let call = root.find("Body").and_then(|b| b.children.first()).ok_or(UpnpError::InvalidAction)?;
    Ok(RawCall {
        action: call.name.clone(),
        args: call.children.iter().map(|c| (c.name.clone(), c.text.clone())).collect(),
    })
}

fn envelope(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body>{body}</s:Body></s:Envelope>"#
    )
}

pub fn response(service: Service, action: &str, out: &[(&str, String)]) -> String {
    let args: String = out.iter().map(|(k, v)| format!("<{k}>{}</{k}>", escape(v))).collect();
    envelope(&format!(r#"<u:{action}Response xmlns:u="{urn}">{args}</u:{action}Response>"#, urn = service.urn()))
}

pub fn fault(error: &UpnpError) -> String {
    envelope(&format!(
        r#"<s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{}</errorCode><errorDescription>{}</errorDescription></UPnPError></detail></s:Fault>"#,
        error.code(),
        escape(&error.to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_request() {
        let body = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <InstanceID>0</InstanceID>
      <CurrentURI>http://192.0.2.10:8080/v.mp4?a=1&amp;b=2</CurrentURI>
      <CurrentURIMetaData>&lt;DIDL-Lite&gt;&lt;/DIDL-Lite&gt;</CurrentURIMetaData>
    </u:SetAVTransportURI>
  </s:Body>
</s:Envelope>"#;
        let call = parse_request(body).unwrap();
        assert_eq!(call.action, "SetAVTransportURI");
        assert_eq!(call.arg("CurrentURI"), Some("http://192.0.2.10:8080/v.mp4?a=1&b=2"));
        assert_eq!(call.arg("CurrentURIMetaData"), Some("<DIDL-Lite></DIDL-Lite>"));
    }

    #[test]
    fn fault_carries_the_code() {
        let f = fault(&UpnpError::TransitionNotAvailable);
        assert!(f.contains("<errorCode>701</errorCode>"));
    }
}
