//! What a player found out about the media it plays: resolution, codecs,
//! frame rate, bitrates, audio format. Every player fills what it can (mpv
//! knows almost everything, a browser less), reports it with
//! `PlayerEvent::Info`, and may report again as it learns more (an HLS stream
//! switching quality); the session merges the reports.
//!
//! JSON (camelCase, unknown parts left out):
//! `{"container":"hls","bitrateBps":5200000,"video":{"codec":"h264","width":1920,"height":1080,"fps":23.976},"audio":{"codec":"aac","sampleRateHz":48000,"channels":2}}`

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    /// Codec, lower case as players name it: "h264", "hevc", "av1", "vp9".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    /// "HDR10", "HLG", "Dolby Vision"; absent for SDR or unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<String>,
    /// e.g. "yuv420p10" (10-bit), as the player reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    /// "aac", "opus", "eac3", "flac", ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate_hz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// "hls", "dash", "mp4", "matroska", "mpegts", ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// The whole stream's bitrate (an HLS variant's, say), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    /// The decoder in use: a hardware one ("videotoolbox", "d3d11va", "mediacodec")
    /// or "software".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioInfo>,
}

macro_rules! take_newer {
    ($old:expr, $new:expr, $($field:ident),*) => {
        $( if $new.$field.is_some() { $old.$field = $new.$field.clone(); } )*
    };
}

impl MediaInfo {
    pub fn is_empty(&self) -> bool {
        *self == MediaInfo::default()
    }

    /// Takes every field `newer` knows; keeps the rest.
    pub fn merge(&mut self, newer: &MediaInfo) {
        take_newer!(self, newer, container, bitrate_bps, decoder);
        if let Some(v) = &newer.video {
            let old = self.video.get_or_insert_with(VideoInfo::default);
            take_newer!(old, v, codec, width, height, fps, bitrate_bps, hdr, pixel_format);
        }
        if let Some(a) = &newer.audio {
            let old = self.audio.get_or_insert_with(AudioInfo::default);
            take_newer!(old, a, codec, sample_rate_hz, channels, bitrate_bps);
        }
    }

    /// Whether `self` is worth reporting after `before`: anything but a
    /// bitrate changed, or a bitrate moved by more than a quarter (bitrates
    /// wobble every second; quality switches don't).
    pub fn differs_notably(&self, before: &MediaInfo) -> bool {
        let strip = |i: &MediaInfo| {
            let mut i = i.clone();
            i.bitrate_bps = None;
            if let Some(v) = &mut i.video {
                v.bitrate_bps = None;
            }
            if let Some(a) = &mut i.audio {
                a.bitrate_bps = None;
            }
            i
        };
        let moved = |a: Option<u64>, b: Option<u64>| match (a, b) {
            (Some(a), Some(b)) => a.abs_diff(b) * 4 > b,
            (a, b) => a.is_some() != b.is_some(),
        };
        strip(self) != strip(before)
            || moved(self.bitrate_bps, before.bitrate_bps)
            || moved(self.video.as_ref().and_then(|v| v.bitrate_bps), before.video.as_ref().and_then(|v| v.bitrate_bps))
            || moved(self.audio.as_ref().and_then(|a| a.bitrate_bps), before.audio.as_ref().and_then(|a| a.bitrate_bps))
    }

    /// The usual name for a picture size: "4K", "1080p", "720p", "SD".
    pub fn resolution_label(&self) -> Option<&'static str> {
        let v = self.video.as_ref()?;
        let (w, h) = (v.width?, v.height?);
        // By the longer edge's class, so 1920×800 (cinema crop) is still 1080p.
        Some(match (w, h) {
            (w, h) if w >= 3800 || h >= 2100 => "4K",
            (w, h) if w >= 2500 || h >= 1400 => "1440p",
            (w, h) if w >= 1900 || h >= 1000 => "1080p",
            (w, h) if w >= 1200 || h >= 700 => "720p",
            (_, h) if h >= 470 => "480p",
            _ => "SD",
        })
    }

    /// A short badge: the resolution and, if any, the HDR format ("1080p",
    /// "4K · HDR10"). `None` until the size is known.
    pub fn short_label(&self) -> Option<String> {
        let res = self.resolution_label()?;
        Some(match self.video.as_ref().and_then(|v| v.hdr.as_deref()) {
            Some(hdr) => format!("{res} · {hdr}"),
            None => res.to_owned(),
        })
    }

    /// One line for people, industry style:
    /// "1080p · 1920×1080 · H.264 · 23.98 fps · 5.2 Mb/s | AAC · 48 kHz · stereo · 128 kb/s".
    pub fn summary(&self) -> String {
        let mut video = Vec::new();
        if let Some(v) = &self.video {
            if let (Some(w), Some(h)) = (v.width, v.height) {
                video.extend(self.resolution_label().map(str::to_owned));
                video.push(format!("{w}×{h}"));
            }
            video.extend(v.codec.as_deref().map(codec_name));
            video.extend(v.hdr.clone());
            video.extend(v.fps.filter(|f| *f > 0.0).map(|f| format!("{} fps", trim_float(f))));
            video.extend(v.bitrate_bps.or(self.bitrate_bps).map(rate));
        } else {
            video.extend(self.bitrate_bps.map(rate));
        }
        let mut audio = Vec::new();
        if let Some(a) = &self.audio {
            audio.extend(a.codec.as_deref().map(codec_name));
            audio.extend(a.sample_rate_hz.map(|r| format!("{} kHz", trim_float(f64::from(r) / 1000.0))));
            audio.extend(a.channels.map(channels));
            audio.extend(a.bitrate_bps.map(rate));
        }
        match (video.is_empty(), audio.is_empty()) {
            (false, false) => format!("{} | {}", video.join(" · "), audio.join(" · ")),
            (false, true) => video.join(" · "),
            (true, false) => audio.join(" · "),
            (true, true) => String::new(),
        }
    }
}

/// "h264" → "H.264", as people write codecs.
pub fn codec_name(codec: &str) -> String {
    match codec.to_ascii_lowercase().as_str() {
        "h264" | "avc" | "avc1" => "H.264".into(),
        "hevc" | "h265" | "hvc1" | "hev1" => "HEVC".into(),
        "av1" | "av01" => "AV1".into(),
        "vp9" | "vp09" => "VP9".into(),
        "vp8" => "VP8".into(),
        "mpeg2video" => "MPEG-2".into(),
        "aac" | "mp4a" => "AAC".into(),
        "ac3" => "AC-3".into(),
        "eac3" | "ec-3" => "E-AC-3".into(),
        "mp3" | "mp3float" => "MP3".into(),
        "opus" => "Opus".into(),
        "flac" => "FLAC".into(),
        "truehd" => "TrueHD".into(),
        "dts" => "DTS".into(),
        other => other.to_uppercase(),
    }
}

/// 5_200_000 → "5.2 Mb/s", 128_000 → "128 kb/s".
pub fn rate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{} Mb/s", trim_float((bps as f64 / 100_000.0).round() / 10.0))
    } else {
        format!("{} kb/s", (bps as f64 / 1000.0).round())
    }
}

fn channels(n: u16) -> String {
    match n {
        1 => "mono".into(),
        2 => "stereo".into(),
        6 => "5.1".into(),
        8 => "7.1".into(),
        n => format!("{n} ch"),
    }
}

/// 23.976 → "23.98", 48.0 → "48", 44.1 → "44.1".
fn trim_float(x: f64) -> String {
    let s = format!("{x:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hls_1080() -> MediaInfo {
        MediaInfo {
            container: Some("hls".into()),
            bitrate_bps: Some(5_200_000),
            decoder: None,
            video: Some(VideoInfo {
                codec: Some("h264".into()),
                width: Some(1920),
                height: Some(1080),
                fps: Some(23.976),
                ..Default::default()
            }),
            audio: Some(AudioInfo {
                codec: Some("aac".into()),
                sample_rate_hz: Some(48_000),
                channels: Some(2),
                bitrate_bps: Some(128_000),
            }),
        }
    }

    #[test]
    fn reads_like_a_player_overlay() {
        assert_eq!(
            hls_1080().summary(),
            "1080p · 1920×1080 · H.264 · 23.98 fps · 5.2 Mb/s | AAC · 48 kHz · stereo · 128 kb/s"
        );
        let cinema = MediaInfo {
            video: Some(VideoInfo { width: Some(3840), height: Some(1600), ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(cinema.resolution_label(), Some("4K"));
        assert_eq!(rate(44_100), "44 kb/s");
        assert_eq!(MediaInfo::default().summary(), "");
        assert_eq!(hls_1080().short_label().as_deref(), Some("1080p"));
        let mut hdr = cinema.clone();
        hdr.video.as_mut().unwrap().hdr = Some("HDR10".into());
        assert_eq!(hdr.short_label().as_deref(), Some("4K · HDR10"));
    }

    #[test]
    fn later_reports_fill_in_and_override() {
        let mut info = MediaInfo {
            video: Some(VideoInfo { codec: Some("h264".into()), ..Default::default() }),
            ..Default::default()
        };
        info.merge(&MediaInfo {
            video: Some(VideoInfo { width: Some(1280), height: Some(720), ..Default::default() }),
            ..Default::default()
        });
        info.merge(&MediaInfo { bitrate_bps: Some(2_000_000), ..Default::default() });
        let v = info.video.as_ref().unwrap();
        assert_eq!((v.codec.as_deref(), v.width, v.height), (Some("h264"), Some(1280), Some(720)));
        assert_eq!(info.bitrate_bps, Some(2_000_000));
        let json = serde_json::to_string(&hls_1080()).unwrap();
        assert!(json.contains(r#""sampleRateHz":48000"#) && json.contains(r#""bitrateBps":5200000"#), "{json}");
    }

    #[test]
    fn only_notable_changes_are_reported() {
        let a = hls_1080();
        let mut wobble = a.clone();
        wobble.bitrate_bps = Some(5_500_000);
        assert!(!wobble.differs_notably(&a), "a 6% bitrate wobble isn't news");
        let mut switch = a.clone();
        switch.bitrate_bps = Some(2_000_000);
        assert!(switch.differs_notably(&a), "a quality switch is");
        let mut size = a.clone();
        size.video.as_mut().unwrap().height = Some(720);
        assert!(size.differs_notably(&a));
        assert!(a.differs_notably(&MediaInfo::default()));
    }
}
