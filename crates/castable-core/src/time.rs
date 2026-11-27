//! UPnP time strings: `H+:MM:SS[.F+]`, e.g. `0:01:30` or `01:02:03.500`.

use std::time::Duration;

pub fn parse_hms(s: &str) -> Option<Duration> {
    let s = s.trim();
    let mut parts = s.split(':');
    let (h, m, sec) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let h: u64 = h.trim_start_matches('+').parse().ok()?;
    let m: u64 = m.parse().ok()?;
    let secs: f64 = sec.parse().ok()?;
    if m >= 60 || !(0.0..60.0).contains(&secs) {
        return None;
    }
    Some(Duration::from_secs(h * 3600 + m * 60) + Duration::from_secs_f64(secs))
}

/// Formats as `H:MM:SS` (UPnP's usual form, whole seconds).
pub fn format_hms(d: Duration) -> String {
    let t = d.as_secs();
    format!("{}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
}

/// Serde helpers: a `Duration` as seconds (`f64`) in JSON.
pub mod secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let v = f64::deserialize(d)?;
        Duration::try_from_secs_f64(v).map_err(serde::de::Error::custom)
    }
}

/// Serde helpers: an optional `Duration` as seconds.
pub mod opt_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_f64(d.as_secs_f64()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Option::<f64>::deserialize(d)?
            .map(|v| Duration::try_from_secs_f64(v).map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        assert_eq!(parse_hms("0:01:30"), Some(Duration::from_secs(90)));
        assert_eq!(parse_hms("01:02:03.500"), Some(Duration::from_millis(3_723_500)));
        assert_eq!(format_hms(Duration::from_secs(3723)), "1:02:03");
        assert_eq!(parse_hms("1:60:00"), None);
        assert_eq!(parse_hms("NOT_IMPLEMENTED"), None);
    }
}
