//! The receiver's log file, and the report a user can attach to an issue.
//!
//! Every run writes a detailed JSON log (what each phone asked, what the
//! player was told and printed, every error with its code) to
//! `logs/receiver.log` in Castable's config folder; the previous run's log is
//! kept as `receiver.1.log`. `castable-receiver --report` (or `castable
//! report`) turns them into one Markdown file with private parts redacted.

use crate::settings::{config_dir, Settings};
use crate::spec::Registry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many log lines a report quotes from the end of each log.
const LAST_LINES: usize = 300;

/// `logs/` in Castable's config folder.
pub fn log_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("logs"))
}

/// Opens this run's log file, keeping the previous run's as `receiver.1.log`.
pub fn open_log(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        let _ = std::fs::rename(path, path.with_extension("1.log"));
    }
    std::fs::File::create(path)
}

/// Hides what a report shouldn't carry: URL credentials (`user:password@`),
/// query strings and fragments (they often hold tokens), passwords and
/// authorization headers.
pub fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    // URLs: keep scheme, host and path; replace ?query and #fragment.
    while let Some(i) = rest.find("://") {
        let start = rest[..i]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'))
            .map_or(0, |p| p + 1);
        out.push_str(&rest[..start]);
        let url_end = rest[i..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | ',' | '|'))
            .map_or(rest.len(), |e| i + e);
        let mut url = &rest[start..url_end];
        // user:password@host: keep the scheme and host, hide the credentials.
        let scheme_end = i - start + 3;
        let authority_end = url[scheme_end..].find(['/', '?', '#']).map_or(url.len(), |e| scheme_end + e);
        if let Some(at) = url[..authority_end].rfind('@') {
            out.push_str(&url[..scheme_end]);
            out.push_str("…@");
            url = &url[at + 1..];
        }
        match url.find(['?', '#']) {
            Some(q) => {
                out.push_str(&url[..q]);
                out.push_str("?…");
            }
            None => out.push_str(url),
        }
        rest = &rest[url_end..];
    }
    out.push_str(rest);
    // key=value secrets, wherever they appear.
    for key in ["password", "--http-password", "authorization", "cookie", "token"] {
        out = redact_value(&out, key);
    }
    out
}

fn redact_value(line: &str, key: &str) -> String {
    // ASCII only: full Unicode lowercasing can change byte lengths (İ), and the
    // offsets found here slice `line` itself.
    let lower = line.to_ascii_lowercase();
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(pos) = lower[from..].find(key) {
        let k = from + pos + key.len();
        // the separator: =, :, or ":" in JSON
        let after = &line[k..];
        let sep = after.find(|c: char| !matches!(c, '"' | ':' | '=' | ' ')).unwrap_or(after.len());
        if sep == 0 || !after[..sep].contains([':', '=']) {
            from = k;
            continue;
        }
        let value_start = k + sep;
        // A quoted value ends at its quote; a bare one at a space or separator.
        let quoted = after[..sep].ends_with('"');
        let value_end = line[value_start..]
            .find(
                |c: char| if quoted { c == '"' } else { c.is_whitespace() || matches!(c, '"' | ',' | '|' | '}' | ';') },
            )
            .map_or(line.len(), |e| value_start + e);
        out.push_str(&line[last..value_start]);
        out.push('…');
        last = value_end;
        from = value_end;
    }
    out.push_str(&line[last..]);
    out
}

/// Error codes seen in a JSON log, with how often: `{"E403": 2, ...}`.
pub fn error_counts(log: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for line in log.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        if let (Some(code), Some(name)) = (v["code"].as_str(), v["error"].as_str()) {
            if code.starts_with('E') {
                *counts.entry(format!("{code} {name}")).or_default() += 1;
            }
        }
    }
    counts
}

fn last_lines(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let tail = &lines[lines.len().saturating_sub(LAST_LINES)..];
    Some(tail.iter().map(|l| redact(l)).collect::<Vec<_>>().join("\n"))
}

/// The report, as Markdown.
pub fn build(settings: &Settings, settings_path: Option<&Path>, registry: &Registry, log: &Path) -> String {
    let mut md = String::new();
    let mut line = |s: String| {
        md.push_str(&s);
        md.push('\n');
    };
    line("# Castable report".into());
    line(String::new());
    line("Attach this file to an issue at https://github.com/vincecao/castable-core/issues.".into());
    line("URL queries, passwords, tokens and cookies are replaced with `…`; check before sharing.".into());
    line(String::new());
    line("## System".into());
    line(format!("- castable-receiver {}", env!("CARGO_PKG_VERSION")));
    line(format!("- {} {} ({})", std::env::consts::OS, std::env::consts::ARCH, std::env::consts::FAMILY));
    line(format!("- settings: {}", settings_path.map_or("(none)".into(), |p| p.display().to_string())));
    line(format!(
        "- name {:?}, player {:?}, port {:?}, log {:?}, font {:?}",
        settings.name.as_deref().unwrap_or("(default)"),
        settings.player.as_deref().unwrap_or("log"),
        settings.port,
        settings.log,
        settings.font().unwrap_or_else(|| "(the player's own)".into()),
    ));
    line(String::new());
    line("## Players".into());
    for (name, spec) in &registry.players {
        let here = if !spec.runs_here() {
            "not for this OS".into()
        } else {
            spec.resolve_program().map_or("not installed".into(), |p| format!("installed: {}", p.display()))
        };
        line(format!("- {name} ({:?}): {here}", spec.control));
    }
    for (title, path) in [("This run", log.to_path_buf()), ("Previous run", log.with_extension("1.log"))] {
        line(String::new());
        let Some(text) = last_lines(&path) else {
            line(format!("## {title}: no log at {}", path.display()));
            continue;
        };
        let counts = error_counts(&text);
        line(format!("## {title}"));
        line(String::new());
        if counts.is_empty() {
            line("No errors.".into());
        } else {
            line("| Error | Times |".into());
            line("| --- | --- |".into());
            for (code, n) in counts {
                line(format!("| {code} | {n} |"));
            }
        }
        line(String::new());
        line(format!("<details><summary>Last {LAST_LINES} log lines ({})</summary>", path.display()));
        line(String::new());
        line("```".into());
        line(text);
        line("```".into());
        line("</details>".into());
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_in_urls_and_non_ascii_text_stay_redacted() {
        assert_eq!(
            redact("load https://user:secret@media.example.com/a.m3u8?token=1 done"),
            "load https://…@media.example.com/a.m3u8?… done"
        );
        // Characters whose lowercase is longer (İ) don't shift where a secret is found…
        assert_eq!(redact("İ password=secret"), "İ password=…");
        // …or make the slicing panic.
        assert_eq!(redact("İ password中文=x"), "İ password中文=x");
        assert_eq!(redact("标题 İ cookie: abc; token=xyz"), "标题 İ cookie: …; token=…");
    }

    #[test]
    fn redacts_tokens_but_keeps_what_helps() {
        let line = r#"{"message":"player command","url":"https://cdn.example.com/hls/6b09e9cc87066.m3u8?sign=abc123&t=99","title":"Sample Movie"}"#;
        let r = redact(line);
        assert!(r.contains("https://cdn.example.com/hls/6b09e9cc87066.m3u8?…"), "{r}");
        assert!(!r.contains("abc123"));
        assert!(r.contains("Sample Movie"));
        let args = "--http-port=4242 --http-password=s3cret --meta-title=A";
        assert_eq!(redact(args), "--http-port=4242 --http-password=… --meta-title=A");
        assert_eq!(redact(r#"{"authorization":"Basic abc"}"#), r#"{"authorization":"…"}"#);
        assert_eq!(redact("no secrets here"), "no secrets here");
    }

    #[test]
    fn counts_error_codes() {
        let log = r#"{"message":"player event","event":"load_failed","code":"E403","error":"forbidden"}
{"message":"control refused","code":"E604","error":"not_now"}
{"message":"player event","event":"load_failed","code":"E403","error":"forbidden"}
not json"#;
        let c = error_counts(log);
        assert_eq!(c.get("E403 forbidden"), Some(&2));
        assert_eq!(c.get("E604 not_now"), Some(&1));
    }
}
