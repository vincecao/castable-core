//! The media descriptor: the one type every product shares.
//! Sources produce it, the cast library sends it, the receiver plays it.
//! Mirrored in TypeScript by `packages/descriptor`.

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
}
