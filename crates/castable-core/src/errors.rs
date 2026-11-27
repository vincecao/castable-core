//! Every error Castable can report, in one catalog.
//!
//! A [`CastError`] is a [`ErrorCode`] plus, optionally, the source's own words
//! (`detail`: a player's output line, an OS error). Codes are grouped into
//! [`ErrorCategory`] buckets; a code's number starts with its bucket's digit,
//! so `E403` is a media error wherever it appears: a log line, the terminal,
//! a receiver screen, a bug report. Players, the network layer and the
//! receiver all map their own failures onto these codes, so the same problem
//! reads the same whichever player or platform hit it.
//!
//! **Adding a code** is one line in the `codes!` table below. Screens show a
//! per-category message for any code they don't know yet, so nothing else
//! has to change at once.
//!
//! JSON: `{"code":"forbidden","id":"E403","category":"media","detail":"HTTP error 403 Forbidden"}`
//! (`id` and `category` are derived from `code`, for readers that don't have
//! this table; only `code` and `detail` are read back).

use crate::error::UpnpError;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// The buckets. The digit is the first digit of every code in the bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// The player program: missing, crashed, not answering.
    Player,
    /// Reaching the media's server.
    Network,
    /// The media itself: refused, gone, unsupported, damaged.
    Media,
    /// A sender's command the receiver can't carry out now (UPnP faults).
    Control,
    /// Castable's own settings and player definitions.
    Config,
    /// This computer: ports, network interfaces, files.
    System,
    Unknown,
}

impl ErrorCategory {
    pub fn digit(self) -> u16 {
        match self {
            Self::Player => 1,
            Self::Network => 2,
            Self::Media => 4,
            Self::Control => 6,
            Self::Config => 7,
            Self::System => 8,
            Self::Unknown => 9,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Network => "network",
            Self::Media => "media",
            Self::Control => "control",
            Self::Config => "config",
            Self::System => "system",
            Self::Unknown => "unknown",
        }
    }
}

/// Declares the codes: `Variant = number, category, "json_name", "English summary";`
macro_rules! codes {
    ($( $(#[$doc:meta])* $variant:ident = $num:literal, $cat:ident, $name:literal, $summary:literal; )*) => {
        /// One kind of error. See the table for each code's number and bucket.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum ErrorCode {
            $( $(#[$doc])* #[serde(rename = $name)] $variant, )*
        }

        impl ErrorCode {
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            /// The stable number, e.g. 403. Its first digit is the category's.
            pub fn number(self) -> u16 {
                match self { $(ErrorCode::$variant => $num,)* }
            }

            pub fn category(self) -> ErrorCategory {
                match self { $(ErrorCode::$variant => ErrorCategory::$cat,)* }
            }

            /// The JSON name, e.g. "forbidden".
            pub fn name(self) -> &'static str {
                match self { $(ErrorCode::$variant => $name,)* }
            }

            /// One English sentence for logs and the terminal. Screens show
            /// their own translated messages.
            pub fn summary(self) -> &'static str {
                match self { $(ErrorCode::$variant => $summary,)* }
            }
        }
    };
}

codes! {
    // ---- Player (1xx)
    PlayerMissing = 101, Player, "player_missing", "the player isn't installed or can't start";
    PlayerCrashed = 102, Player, "player_crashed", "the player quit with an error";
    PlayerUnresponsive = 103, Player, "player_unresponsive", "the player stopped answering";
    // ---- Network (2xx)
    Network = 201, Network, "network", "the video's server couldn't be reached, or the connection dropped";
    Timeout = 202, Network, "timeout", "the video's server took too long to answer";
    // ---- Media (4xx)
    /// HTTP 401/403: the stream often needs a Referer, User-Agent or login
    /// that the sender didn't pass on.
    Forbidden = 403, Media, "forbidden", "the video's server refused it (it may only play in its own app)";
    NotFound = 404, Media, "not_found", "the video isn't there any more (the link may have expired)";
    Unsupported = 415, Media, "unsupported", "the player can't play this format";
    Decode = 422, Media, "decode", "the video is damaged or couldn't be decoded";
    // ---- Control (6xx): UPnP faults sent back to a phone
    InvalidAction = 601, Control, "invalid_action", "the phone asked for something this receiver doesn't do";
    InvalidArgs = 602, Control, "invalid_args", "the phone sent a command with missing or bad values";
    ActionFailed = 603, Control, "action_failed", "the command failed";
    NotNow = 604, Control, "not_now", "that command doesn't apply right now (nothing is loaded, say)";
    SeekNotSupported = 605, Control, "seek_not_supported", "this video can't be seeked (a live stream)";
    SeekOutOfRange = 606, Control, "seek_out_of_range", "the seek goes past the end of the video";
    MimeNotAccepted = 607, Control, "mime_not_accepted", "the receiver doesn't accept this kind of media";
    InvalidInstance = 608, Control, "invalid_instance", "the phone addressed a player this receiver doesn't have";
    // ---- Config (7xx)
    SettingsInvalid = 701, Config, "settings_invalid", "the settings file couldn't be read";
    PlayersFileInvalid = 702, Config, "players_file_invalid", "a players.toml entry is wrong";
    PlayerUnknown = 703, Config, "player_unknown", "there's no player with that name";
    PlayerNotForThisOs = 704, Config, "player_not_for_this_os", "that player doesn't run on this computer's OS";
    BadArgument = 705, Config, "bad_argument", "a command-line argument is wrong";
    // ---- System (8xx)
    PortInUse = 801, System, "port_in_use", "the port is already in use by another program";
    NoNetwork = 802, System, "no_network", "this computer has no network that phones can reach";
    Io = 803, System, "io", "a file or folder couldn't be read or written";
    // ---- Unknown (9xx)
    Unknown = 999, Unknown, "unknown", "something went wrong";
}

impl ErrorCode {
    /// "E403".
    pub fn id(self) -> String {
        format!("E{}", self.number())
    }
}

/// UPnP faults map one to one onto the control bucket.
impl From<&UpnpError> for ErrorCode {
    fn from(e: &UpnpError) -> Self {
        match e {
            UpnpError::InvalidAction => Self::InvalidAction,
            UpnpError::InvalidArgs => Self::InvalidArgs,
            UpnpError::ActionFailed => Self::ActionFailed,
            UpnpError::TransitionNotAvailable => Self::NotNow,
            UpnpError::SeekModeNotSupported => Self::SeekNotSupported,
            UpnpError::IllegalSeekTarget => Self::SeekOutOfRange,
            UpnpError::IllegalMimeType => Self::MimeNotAccepted,
            UpnpError::InvalidInstanceId => Self::InvalidInstance,
        }
    }
}

/// An error, with its source's own words if it gave any.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CastError {
    pub code: ErrorCode,
    #[serde(default)]
    pub detail: Option<String>,
}

impl CastError {
    pub fn new(code: ErrorCode) -> Self {
        Self { code, detail: None }
    }

    /// `detail` is dropped if blank.
    pub fn with_detail(code: ErrorCode, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self { code, detail: (!detail.trim().is_empty()).then_some(detail) }
    }
}

impl Serialize for CastError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            code: ErrorCode,
            id: String,
            category: ErrorCategory,
            #[serde(skip_serializing_if = "Option::is_none")]
            detail: Option<&'a str>,
        }
        Wire { code: self.code, id: self.code.id(), category: self.code.category(), detail: self.detail.as_deref() }
            .serialize(s)
    }
}

/// "E403 media/forbidden: the video's server refused it (…) — HTTP error 403 Forbidden"
impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code;
        write!(f, "{} {}/{}: {}", c.id(), c.category().name(), c.name(), c.summary())?;
        if let Some(d) = &self.detail {
            write!(f, " — {d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CastError {}

impl From<UpnpError> for CastError {
    fn from(e: UpnpError) -> Self {
        CastError::with_detail(ErrorCode::from(&e), format!("UPnP {} {e}", e.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_unique_and_numbered_by_its_bucket() {
        let mut numbers: Vec<_> = ErrorCode::ALL.iter().map(|c| c.number()).collect();
        numbers.sort();
        numbers.dedup();
        assert_eq!(numbers.len(), ErrorCode::ALL.len(), "two codes share a number");
        for &c in ErrorCode::ALL {
            assert_eq!(c.number() / 100, c.category().digit(), "{c:?} is numbered outside its bucket");
            assert_eq!(serde_json::to_value(c).unwrap(), serde_json::json!(c.name()));
        }
    }

    #[test]
    fn reads_the_same_everywhere() {
        let e = CastError::with_detail(ErrorCode::Forbidden, "HTTP error 403 Forbidden");
        assert!(e.to_string().starts_with("E403 media/forbidden: "));
        assert!(e.to_string().ends_with(" — HTTP error 403 Forbidden"));
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#"{"code":"forbidden","id":"E403","category":"media","detail":"HTTP error 403 Forbidden"}"#);
        assert_eq!(serde_json::from_str::<CastError>(&json).unwrap(), e);
        assert_eq!(CastError::with_detail(ErrorCode::Unknown, "  ").detail, None);
    }

    #[test]
    fn upnp_faults_land_in_the_control_bucket() {
        let e = CastError::from(UpnpError::TransitionNotAvailable);
        assert_eq!((e.code, e.code.category()), (ErrorCode::NotNow, ErrorCategory::Control));
        assert_eq!(e.detail.as_deref(), Some("UPnP 701 Transition not available"));
    }
}
