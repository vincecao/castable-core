//! DIDL-Lite metadata that senders attach to `SetAVTransportURI`.
//! Parsed tolerantly: many senders send odd prefixes, missing namespaces or
//! nothing at all, and the cast must still work.

use crate::descriptor::{Delivery, MediaDescriptor, MediaKind};
use crate::xml::parse;

/// Builds the descriptor for a cast from its URI and optional DIDL-Lite.
pub fn descriptor_from(uri: &str, metadata: Option<&str>) -> MediaDescriptor {
    let mut media = MediaDescriptor::from_url_and_mime(uri, None);
    let Some(item) =
        metadata.filter(|m| !m.trim().is_empty()).and_then(parse).and_then(|root| root.find("item").cloned())
    else {
        return guess_from_url(media);
    };
    media.title = item.child_text("title").map(str::to_string).filter(|t| !t.is_empty());
    // protocolInfo is "http-get:*:<mime>:<dlna flags>"
    let res = item
        .children
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case("res") && c.text.trim() == uri)
        .or_else(|| item.child("res"));
    if let Some(mime) = res.and_then(|r| r.attr("protocolInfo")).and_then(|p| p.split(':').nth(2)).filter(|m| *m != "*")
    {
        media.mime = Some(mime.to_string());
    }
    let class = item.child_text("class").unwrap_or_default();
    media.kind = if class.contains("audioItem") {
        MediaKind::Audio
    } else if class.contains("imageItem") {
        MediaKind::Image
    } else {
        kind_from_mime(media.mime.as_deref()).unwrap_or(MediaKind::Video)
    };
    if class.contains("videoBroadcast") || class.contains("audioBroadcast") {
        media.delivery = Delivery::Live;
    }
    guess_from_url(media)
}

fn kind_from_mime(mime: Option<&str>) -> Option<MediaKind> {
    let m = mime?;
    Some(if m.starts_with("audio/") {
        MediaKind::Audio
    } else if m.starts_with("image/") {
        MediaKind::Image
    } else {
        MediaKind::Video
    })
}

/// Fills a missing MIME type from the URL's extension.
fn guess_from_url(mut media: MediaDescriptor) -> MediaDescriptor {
    if media.mime.is_none() {
        let path = media.url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
        let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
        media.mime = match ext {
            "m3u8" => Some("application/vnd.apple.mpegurl"),
            "mp4" | "m4v" => Some("video/mp4"),
            "mov" => Some("video/quicktime"),
            "mkv" => Some("video/x-matroska"),
            "mp3" => Some("audio/mpeg"),
            "m4a" => Some("audio/mp4"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "png" => Some("image/png"),
            _ => None,
        }
        .map(str::to_string);
        if let Some(kind) = kind_from_mime(media.mime.as_deref()) {
            media.kind = kind;
        }
    }
    media
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_title_mime_and_class() {
        let didl = r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="0" parentID="-1" restricted="1"><dc:title>Episode 1</dc:title><upnp:class>object.item.videoItem</upnp:class><res protocolInfo="http-get:*:video/mp4:DLNA.ORG_OP=01">https://media.example.com/e1.mp4</res></item></DIDL-Lite>"#;
        let m = descriptor_from("https://media.example.com/e1.mp4", Some(didl));
        assert_eq!(m.title.as_deref(), Some("Episode 1"));
        assert_eq!(m.mime.as_deref(), Some("video/mp4"));
        assert_eq!(m.kind, MediaKind::Video);
    }

    #[test]
    fn survives_missing_or_broken_metadata() {
        let m = descriptor_from("https://media.example.com/live.m3u8?token=1", None);
        assert_eq!(m.mime.as_deref(), Some("application/vnd.apple.mpegurl"));
        let m = descriptor_from("https://media.example.com/song.mp3", Some("<not xml"));
        assert_eq!(m.kind, MediaKind::Audio);
    }

    #[test]
    fn broadcast_class_is_live() {
        let didl = r#"<DIDL-Lite><item><dc:title>TV</dc:title><upnp:class>object.item.videoItem.videoBroadcast</upnp:class></item></DIDL-Lite>"#;
        assert!(descriptor_from("https://media.example.com/ch1", Some(didl)).is_live());
    }
}
