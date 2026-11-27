//! The device description (`/desc.xml`) and SSDP discovery messages.

use crate::service::Service;
use crate::xml::escape;

pub const DEVICE_TYPE: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
pub const DESCRIPTION_PATH: &str = "/desc.xml";
pub const SSDP_ADDR: &str = "239.255.255.250:1900";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub friendly_name: String,
    /// `uuid:...`, stable across restarts of the same install.
    pub udn: String,
    pub manufacturer: String,
    pub model_name: String,
    pub model_number: String,
    /// The `SERVER` header: `OS/version UPnP/1.0 product/version`.
    pub server: String,
}

impl DeviceInfo {
    pub fn new(friendly_name: impl Into<String>, uuid: impl AsRef<str>) -> Self {
        Self {
            friendly_name: friendly_name.into(),
            udn: format!("uuid:{}", uuid.as_ref().trim_start_matches("uuid:")),
            manufacturer: "Castable".into(),
            model_name: "Castable Receiver".into(),
            model_number: env!("CARGO_PKG_VERSION").into(),
            server: format!("Generic/1.0 UPnP/1.0 Castable/{}", env!("CARGO_PKG_VERSION")),
        }
    }

    pub fn description_xml(&self) -> String {
        let services: String = Service::ALL
            .iter()
            .map(|s| {
                format!(
                    "<service><serviceType>{urn}</serviceType><serviceId>urn:upnp-org:serviceId:{id}</serviceId><SCPDURL>{scpd}</SCPDURL><controlURL>{ctl}</controlURL><eventSubURL>{evt}</eventSubURL></service>",
                    urn = s.urn(),
                    id = s.id(),
                    scpd = s.scpd_path(),
                    ctl = s.control_path(),
                    evt = s.event_path()
                )
            })
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0"><specVersion><major>1</major><minor>0</minor></specVersion><device><deviceType>{DEVICE_TYPE}</deviceType><friendlyName>{name}</friendlyName><manufacturer>{mf}</manufacturer><modelName>{model}</modelName><modelNumber>{num}</modelNumber><UDN>{udn}</UDN><dlna:X_DLNADOC>DMR-1.50</dlna:X_DLNADOC><serviceList>{services}</serviceList></device></root>"#,
            name = escape(&self.friendly_name),
            mf = escape(&self.manufacturer),
            model = escape(&self.model_name),
            num = escape(&self.model_number),
            udn = escape(&self.udn),
        )
    }

    /// Every (NT, USN) pair this device advertises. Used for both M-SEARCH
    /// answers and NOTIFY announcements, so the two can't disagree.
    pub fn targets(&self) -> Vec<(String, String)> {
        let udn = &self.udn;
        let mut v = vec![
            ("upnp:rootdevice".to_string(), format!("{udn}::upnp:rootdevice")),
            (udn.clone(), udn.clone()),
            (DEVICE_TYPE.to_string(), format!("{udn}::{DEVICE_TYPE}")),
        ];
        for s in Service::ALL {
            v.push((s.urn(), format!("{udn}::{}", s.urn())));
        }
        v
    }
}

/// A parsed SSDP M-SEARCH request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub st: String,
    /// Seconds the requester waits; answers should be spread over it.
    pub mx: u32,
}

/// Parses an SSDP datagram. Returns `None` for anything that is not a
/// well-formed discovery M-SEARCH.
pub fn parse_search(packet: &str) -> Option<Search> {
    let mut lines = packet.split("\r\n").flat_map(|l| l.split('\n'));
    if !lines.next()?.trim().eq_ignore_ascii_case("M-SEARCH * HTTP/1.1") {
        return None;
    }
    let (mut st, mut man, mut mx) = (None, None, 1);
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim();
            match k.trim().to_ascii_uppercase().as_str() {
                "ST" => st = Some(v.to_string()),
                "MAN" => man = Some(v.trim_matches('"').to_string()),
                "MX" => mx = v.parse().unwrap_or(1).clamp(1, 5),
                _ => {}
            }
        }
    }
    (man.as_deref() == Some("ssdp:discover")).then_some(Search { st: st?, mx })
}

/// The answers to an M-SEARCH: one HTTP response per matching target.
pub fn search_responses(device: &DeviceInfo, search: &Search, location: &str) -> Vec<String> {
    device
        .targets()
        .into_iter()
        .filter(|(nt, _)| search.st == "ssdp:all" || search.st.eq_ignore_ascii_case(nt))
        .map(|(nt, usn)| {
            format!(
                "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: {location}\r\nSERVER: {server}\r\nST: {nt}\r\nUSN: {usn}\r\nContent-Length: 0\r\n\r\n",
                server = device.server
            )
        })
        .collect()
}

/// `ssdp:alive` or `ssdp:byebye` announcements, one per target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Announce {
    Alive,
    ByeBye,
}

pub fn announcements(device: &DeviceInfo, kind: Announce, location: &str) -> Vec<String> {
    device
        .targets()
        .into_iter()
        .map(|(nt, usn)| match kind {
            Announce::Alive => format!(
                "NOTIFY * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: {server}\r\nUSN: {usn}\r\n\r\n",
                server = device.server
            ),
            Announce::ByeBye => {
                format!("NOTIFY * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nNT: {nt}\r\nNTS: ssdp:byebye\r\nUSN: {usn}\r\n\r\n")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev() -> DeviceInfo {
        DeviceInfo::new("Living room <TV>", "1234")
    }

    #[test]
    fn answers_only_matching_targets() {
        let s = parse_search("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:service:AVTransport:1\r\n\r\n").unwrap();
        assert_eq!(s.mx, 2);
        let r = search_responses(&dev(), &s, "http://10.0.0.2:49152/desc.xml");
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("USN: uuid:1234::urn:schemas-upnp-org:service:AVTransport:1"));
        let all = Search { st: "ssdp:all".into(), mx: 1 };
        assert_eq!(search_responses(&dev(), &all, "x").len(), 6);
    }

    #[test]
    fn ignores_non_discovery() {
        assert_eq!(parse_search("NOTIFY * HTTP/1.1\r\n\r\n"), None);
        assert_eq!(parse_search("M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n"), None);
    }

    #[test]
    fn description_is_valid_and_escaped() {
        let xml = dev().description_xml();
        let root = crate::xml::parse(&xml).unwrap();
        assert_eq!(root.find("friendlyName").unwrap().text, "Living room <TV>");
        assert_eq!(root.find("serviceList").unwrap().children.len(), 3);
    }
}
