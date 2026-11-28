//! The tracks a player found in the media (audio languages, video variants,
//! subtitles), so the screen can offer a choice.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Audio,
    Video,
    Subtitle,
}

/// One track, as the player names it. `id` is the player's own and only
/// means something to that player; the screen sends it back to select it.
/// JSON: `{"id":"2","kind":"audio","label":"English","lang":"en","codec":"aac","channels":6,"selected":true}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub id: String,
    pub kind: TrackKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// BCP 47 or ISO 639 code, as the media says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    #[serde(default)]
    pub selected: bool,
}

/// Marks `id` (or nothing, for `None`: subtitles off, video on automatic) as
/// the selected track of `kind`, as the player will once it applies it.
pub fn select(tracks: &mut [MediaTrack], kind: TrackKind, id: Option<&str>) {
    for t in tracks.iter_mut().filter(|t| t.kind == kind) {
        t.selected = Some(t.id.as_str()) == id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, kind: TrackKind, selected: bool) -> MediaTrack {
        MediaTrack {
            id: id.into(),
            kind,
            label: None,
            lang: None,
            codec: None,
            width: None,
            height: None,
            bitrate_bps: None,
            channels: None,
            selected,
        }
    }

    #[test]
    fn selecting_a_track_changes_only_its_kind() {
        let mut tracks = vec![
            track("1", TrackKind::Audio, true),
            track("2", TrackKind::Audio, false),
            track("1", TrackKind::Video, true),
        ];
        select(&mut tracks, TrackKind::Audio, Some("2"));
        assert_eq!(tracks.iter().map(|t| t.selected).collect::<Vec<_>>(), [false, true, true]);
        select(&mut tracks, TrackKind::Video, None);
        assert!(!tracks[2].selected);
    }

    #[test]
    fn the_wire_format() {
        let json =
            r#"{"id":"2","kind":"audio","label":"English","lang":"en","codec":"aac","channels":6,"selected":true}"#;
        let t: MediaTrack = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&t).unwrap(), json);
    }
}
