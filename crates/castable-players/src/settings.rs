//! The receiver's settings file, written by the setup wizard (`castable setup`)
//! and read by `castable-receiver`. Command-line flags override it.
//!
//! ```toml
//! name = "Living room"
//! player = "mpv"      # a name from players.toml, a command line, or "log"
//! port = 49494        # unset: 49494, or a free one if taken; 0: always a free one
//! log = "pretty"      # or "json"
//! font = "PingFang SC"  # on-screen text in players; "" leaves the player's own
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
    /// `None`: `castable_net::PREFERRED_PORT`, falling back to a free one.
    pub port: Option<u16>,
    pub log: LogFormat,
    /// The font players use for on-screen text and subtitles (`{font}` in
    /// players.toml). Unset: one per OS that covers Chinese, Japanese and
    /// Korean as well as Latin. `""`: leave the player's own choice.
    pub font: Option<String>,
}

impl Settings {
    /// Saves `name` into the settings file at `path`, keeping its other keys.
    /// A rename on the status screen lands here, like the setup wizard's.
    pub fn save_name(path: &std::path::Path, name: &str) -> Result<(), String> {
        let mut table: toml::Table = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        table.insert("name".into(), toml::Value::String(name.to_owned()));
        let text = toml::to_string(&table).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads `path`; a missing file is the defaults.
    pub fn load(path: &std::path::Path) -> Result<Settings, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The font to hand to players, if any.
    pub fn font(&self) -> Option<String> {
        match self.font.as_deref() {
            Some("") => None,
            Some(f) => Some(f.to_owned()),
            None => Some(default_font().to_owned()),
        }
    }
}

/// A system font with CJK and Latin glyphs. mpv's default ("sans-serif")
/// often has no CJK, so Chinese titles and subtitles show as boxes.
pub fn default_font() -> &'static str {
    if cfg!(target_os = "macos") {
        "PingFang SC"
    } else if cfg!(windows) {
        "Microsoft YaHei"
    } else {
        "Noto Sans CJK SC"
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
        assert_eq!((s.port, s.log), (Some(49152), LogFormat::Json));
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
        assert!(toml::from_str::<Settings>("colour = \"red\"").is_err(), "typos are reported");
    }

    #[test]
    fn a_font_by_default_none_when_empty() {
        assert_eq!(Settings::default().font().as_deref(), Some(default_font()));
        let s: Settings = toml::from_str("font = \"Songti SC\"").unwrap();
        assert_eq!(s.font().as_deref(), Some("Songti SC"));
        let s: Settings = toml::from_str("font = \"\"").unwrap();
        assert_eq!(s.font(), None);
    }
}
