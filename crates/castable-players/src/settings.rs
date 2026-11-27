//! The receiver's settings file, written by the setup wizard (`castable setup`)
//! and read by `castable-receiver`. Command-line flags override it.
//!
//! ```toml
//! name = "Living room"
//! player = "mpv"      # a name from players.toml, a command line, or "log"
//! port = 49152        # 0 picks a free port
//! log = "pretty"      # or "json"
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub name: Option<String>,
    pub player: Option<String>,
    pub port: u16,
    pub log: LogFormat,
}

impl Settings {
    /// Reads `path`; a missing file is the defaults.
    pub fn load(path: &std::path::Path) -> Result<Settings, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// Castable's folder for settings and the user's players.toml:
/// `%APPDATA%\castable` on Windows, `$XDG_CONFIG_HOME/castable` or
/// `~/.config/castable` elsewhere (macOS included, so the path is easy to type).
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("castable"));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("castable"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config").join("castable"))
}

/// `$CASTABLE_CONFIG`, else `config.toml` in [`config_dir`].
pub fn settings_file() -> Option<PathBuf> {
    std::env::var_os("CASTABLE_CONFIG").map(PathBuf::from).or_else(|| Some(config_dir()?.join("config.toml")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_wizard_writes() {
        let s: Settings =
            toml::from_str("name = \"Living room\"\nplayer = \"mpv\"\nport = 49152\nlog = \"json\"\n").unwrap();
        assert_eq!(s.name.as_deref(), Some("Living room"));
        assert_eq!((s.port, s.log), (49152, LogFormat::Json));
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
        assert!(toml::from_str::<Settings>("colour = \"red\"").is_err(), "typos are reported");
    }
}
