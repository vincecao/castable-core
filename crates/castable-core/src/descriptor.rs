//! The media descriptor: the one type every product shares.
//! Sources produce it, the cast library sends it, the receiver plays it.
//! Mirrored in TypeScript by `packages/protocol`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What kind of media a descriptor points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

/// How the media is delivered. A discriminated union: a live stream has no
/// duration and can't be seeked; on-demand media may carry a start position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Delivery {
    OnDemand {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_at_secs: Option<f64>,
    },
    Live,
}

impl Default for Delivery {
    fn default() -> Self {
        Delivery::OnDemand { start_at_secs: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subtitle {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDescriptor {
    pub url: String,
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Extra request headers some streams need (Referer, User-Agent).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitles: Vec<Subtitle>,
    #[serde(default)]
    pub delivery: Delivery,
}

impl MediaDescriptor {
    /// A plain on-demand video at `url`.
    pub fn video(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            kind: MediaKind::Video,
            mime: None,
            title: None,
            headers: BTreeMap::new(),
            subtitles: Vec::new(),
            delivery: Delivery::default(),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.delivery, Delivery::Live)
    }

    /// Guess live-ness and kind from a MIME type or URL when the sender
    /// gives no metadata. HLS is treated as on-demand until the player
    /// reports otherwise.
    pub fn from_url_and_mime(url: impl Into<String>, mime: Option<String>) -> Self {
        let url = url.into();
        let kind = match mime.as_deref() {
            Some(m) if m.starts_with("audio/") => MediaKind::Audio,
            Some(m) if m.starts_with("image/") => MediaKind::Image,
            _ => MediaKind::Video,
        };
        Self { kind, mime, ..Self::video(url) }
    }

    /// The server's host (and port, if not the default), for logs and status.
    pub fn host(&self) -> String {
        url::Url::parse(&self.url)
            .ok()
            .and_then(|u| {
                let host = u.host_str()?.to_owned();
                Some(u.port().map_or(host.clone(), |p| format!("{host}:{p}")))
            })
            .unwrap_or_default()
    }

    /// The delivery format, from the MIME type or the URL: "hls", "dash",
    /// "mp4", "webm", "matroska", "mpegts", "mp3"... `None` if unclear.
    pub fn format(&self) -> Option<&'static str> {
        let mime = self.mime.as_deref().unwrap_or("").to_ascii_lowercase();
        let path = self.url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        let ext = path.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.contains('/')).unwrap_or("");
        Some(match (mime.as_str(), ext) {
            (m, _) if m.contains("mpegurl") => "hls",
            (_, "m3u8") => "hls",
            (m, _) if m.contains("dash") => "dash",
            (_, "mpd") => "dash",
            ("video/mp4" | "audio/mp4", _) | (_, "mp4" | "m4v" | "m4a" | "mov") => "mp4",
            ("video/webm" | "audio/webm", _) | (_, "webm") => "webm",
            ("video/x-matroska", _) | (_, "mkv") => "matroska",
            ("video/mp2t", _) | (_, "ts" | "m2ts") => "mpegts",
            ("audio/mpeg", _) | (_, "mp3") => "mp3",
            ("audio/flac", _) | (_, "flac") => "flac",
            _ => return None,
        })
    }

    /// What to call the media on screen: the sender's title, else the URL's
    /// file name decoded (`%E5%90%8E%E4%BC%9A.mp4` → `后会`), else its host.
    /// The UIs (`titleOf` in TypeScript) follow the same rule.
    pub fn display_title(&self) -> String {
        if let Some(t) = self.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            return t.to_owned();
        }
        let file = url::Url::parse(&self.url)
            .ok()
            .and_then(|u| u.path_segments()?.rev().find(|s| !s.is_empty()).map(str::to_owned))
            .map(|f| percent_encoding::percent_decode_str(&f).decode_utf8_lossy().into_owned())
            .unwrap_or_default();
        let stem = match file.rsplit_once('.') {
            Some((stem, ext))
                if !stem.is_empty()
                    && (2..=5).contains(&ext.len())
                    && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
            {
                stem
            }
            _ => file.as_str(),
        };
        if stem.is_empty() {
            self.host()
        } else {
            stem.to_owned()
        }
    }
}

#[cfg(test)]
mod title_tests {
    use super::MediaDescriptor;

    #[test]
    fn a_title_falls_back_to_the_decoded_file_name() {
        let t = |url: &str| MediaDescriptor::video(url).display_title();
        assert_eq!(t("http://x/movies/%E5%90%8E%E4%BC%9A%E5%8F%8B%E6%9C%9F.mp4?t=1"), "后会友期");
        assert_eq!(t("http://x/a%20b/Episode%201.mkv"), "Episode 1");
        assert_eq!(t("http://x/live/index.m3u8"), "index");
        assert_eq!(t("http://10.0.0.5:8080/"), "10.0.0.5:8080");
        assert_eq!(t("http://x/100%"), "100%");
        let mut m = MediaDescriptor::video("http://x/a.mp4");
        m.title = Some("  后会友期 ".into());
        assert_eq!(m.display_title(), "后会友期");
    }

    #[test]
    fn host_and_format_for_logs() {
        let m = MediaDescriptor::video("https://user:pw@cdn.example.com:8443/hls/6b09e9cc87066.m3u8?sign=x");
        assert_eq!((m.host().as_str(), m.format()), ("cdn.example.com:8443", Some("hls")));
        let mut m = MediaDescriptor::video("http://x/play?id=1");
        assert_eq!(m.format(), None);
        m.mime = Some("video/mp4".into());
        assert_eq!(m.format(), Some("mp4"));
    }
}
