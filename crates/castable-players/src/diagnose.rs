//! Turning what a player printed into a [`CastError`], the same way for every
//! player. mpv, IINA, ffplay and VLC mostly print ffmpeg's messages, so one
//! table of phrases covers them; add a row to [`RULES`] for a new phrase.

use castable_net::{CastError, ErrorCode};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Phrases (lower case) that identify an error, checked in order: the first
/// row with a matching phrase wins, so more specific rows come first.
const RULES: &[(&[&str], ErrorCode)] = &[
    (
        &[
            "http error 401",
            "http error 403",
            "401 unauthorized",
            "403 forbidden",
            "server returned 403",
            "server returned 401",
        ],
        ErrorCode::Forbidden,
    ),
    (
        &["http error 404", "http error 410", "404 not found", "server returned 404", "no such file or directory"],
        ErrorCode::NotFound,
    ),
    (&["timed out", "timeout", "operation too slow"], ErrorCode::Timeout),
    (
        &[
            "connection refused",
            "connection reset",
            "network is unreachable",
            "could not resolve",
            "failed to resolve",
            "name or service not known",
            "nodename nor servname",
            "no route to host",
            "ssl error",
            "tls error",
            "certificate",
            "http error 5",
            "server returned 5",
            "network error",
        ],
        ErrorCode::Network,
    ),
    (
        &[
            "unrecognized file format",
            "failed to recognize file format",
            "no video or audio streams",
            "no audio or video data played",
            "unsupported",
            "codec not found",
            "no decoder",
            "your input can't be opened",
        ],
        ErrorCode::Unsupported,
    ),
    (&["invalid data found", "error while decoding", "corrupt", "decoding error"], ErrorCode::Decode),
];

/// The code a line of player output points to, if any.
pub fn classify(line: &str) -> Option<ErrorCode> {
    let line = line.to_lowercase();
    RULES.iter().find(|(phrases, _)| phrases.iter().any(|p| line.contains(p))).map(|&(_, code)| code)
}

/// How many of a player's last output lines are kept.
const KEEP: usize = 5;

/// A player's last meaningful output lines, shared by the reader tasks that
/// fill it and the code that reports a failure.
#[derive(Debug, Clone, Default)]
pub struct Tail(Arc<Mutex<VecDeque<String>>>);

impl Tail {
    pub fn push(&self, line: String) {
        if let Ok(mut t) = self.0.lock() {
            if t.len() == KEEP {
                t.pop_front();
            }
            t.push_back(line);
        }
    }

    pub fn lines(&self) -> Vec<String> {
        self.0.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default()
    }

    /// An error from what the player said: the newest line that names a known
    /// error sets the code and is the detail; otherwise `fallback` with the
    /// last lines as detail.
    pub fn diagnose(&self, fallback: ErrorCode) -> CastError {
        let lines = self.lines();
        match lines.iter().rev().find_map(|l| classify(l).map(|code| (code, l))) {
            Some((code, line)) => CastError::with_detail(code, line.clone()),
            None => CastError::with_detail(fallback, lines.join(" · ")),
        }
    }

    /// Like [`Tail::diagnose`], but keeps `error` unless the output is more
    /// specific (a player's own event often says only "loading failed").
    pub fn refine(&self, error: CastError) -> CastError {
        match self.diagnose(error.code) {
            better if better.code != error.code => better,
            _ => error,
        }
    }
}

/// A line worth keeping: no terminal colours, no blank or status lines
/// (mpv's "(Paused) AV: 00:00:01 / ...", "Exiting... (Errors when loading file)").
pub fn meaningful(line: &str) -> Option<String> {
    let last = line.split('\r').next_back().unwrap_or("");
    let clean = strip_ansi_escapes::strip_str(last);
    let clean = clean.trim();
    let status = ["(Paused)", "AV:", "A:", "V:", "Exiting...", "Playing:", "(+)"];
    (!clean.is_empty() && !status.iter().any(|s| clean.starts_with(s))).then(|| clean.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_output_maps_to_codes() {
        let cases = [
            ("[ffmpeg] https: HTTP error 403 Forbidden", Some(ErrorCode::Forbidden)),
            ("[ffmpeg/demuxer] hls: HTTP error 404 Not Found", Some(ErrorCode::NotFound)),
            ("[ffmpeg] tcp: Connection timed out", Some(ErrorCode::Timeout)),
            (
                "[ffmpeg] tcp: Failed to resolve hostname cdn.example.com: nodename nor servname provided",
                Some(ErrorCode::Network),
            ),
            ("Failed to recognize file format.", Some(ErrorCode::Unsupported)),
            ("main: Your input can't be opened", Some(ErrorCode::Unsupported)),
            ("[ffmpeg/video] h264: Invalid data found when processing input", Some(ErrorCode::Decode)),
            ("Playing: https://media.example.com/a.m3u8", None),
        ];
        for (line, code) in cases {
            assert_eq!(classify(line), code, "{line}");
        }
    }

    #[test]
    fn the_newest_known_line_wins() {
        let t = Tail::default();
        for l in [
            "Playing: https://media.example.com/a.m3u8",
            "[ffmpeg] https: HTTP error 403 Forbidden",
            "Failed to open https://media.example.com/a.m3u8.",
        ] {
            t.push(l.into());
        }
        let e = t.diagnose(ErrorCode::PlayerCrashed);
        assert_eq!(
            (e.code, e.detail.as_deref()),
            (ErrorCode::Forbidden, Some("[ffmpeg] https: HTTP error 403 Forbidden"))
        );
        let quiet = Tail::default().diagnose(ErrorCode::PlayerCrashed);
        assert_eq!((quiet.code, quiet.detail), (ErrorCode::PlayerCrashed, None));
        let vague = CastError::with_detail(ErrorCode::Unknown, "loading failed");
        assert_eq!(t.refine(vague).code, ErrorCode::Forbidden);
    }

    #[test]
    fn status_lines_are_dropped() {
        assert_eq!(
            meaningful("\x1b[31mFailed to open https://media.example.com/a.m3u8.\x1b[0m").as_deref(),
            Some("Failed to open https://media.example.com/a.m3u8.")
        );
        assert_eq!(meaningful("(Paused) AV: 00:00:01 / 00:10:00 (0%)"), None);
        assert_eq!(meaningful("Exiting... (Errors when loading file)"), None);
        assert_eq!(meaningful("   "), None);
    }
}
