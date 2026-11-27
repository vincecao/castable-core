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

    /// What to call the media on screen: the sender's title, else the URL's
    /// file name decoded (`%E5%90%8E%E4%BC%9A.mp4` → `后会`), else its host.
    /// The UIs (`titleOf` in TypeScript) follow the same rule.
    pub fn display_title(&self) -> String {
        if let Some(t) = self.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            return t.to_owned();
        }
        let rest = self.url.split_once("://").map_or(self.url.as_str(), |(_, r)| r);
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let path = path.split(['?', '#']).next().unwrap_or("");
        let file = percent_decode(path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(""));
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
            host.to_owned()
        } else {
            stem.to_owned()
        }
    }
}

/// `%XX` escapes to bytes, read as UTF-8 (invalid sequences become U+FFFD).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).and_then(|&b| hex(b)), bytes.get(i + 2).and_then(|&b| hex(b))) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
}
