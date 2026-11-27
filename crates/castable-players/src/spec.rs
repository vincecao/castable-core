//! The player registry: `players.toml` read into typed specs.

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// The players that ship with Castable.
pub const BUILT_IN: &str = include_str!("../players.toml");

/// How the receiver talks to a running player. Each variant is one protocol
/// module; every player that speaks it needs only a TOML entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    /// mpv's JSON IPC socket (mpv, IINA, and other mpv front-ends).
    MpvIpc,
    /// VLC's web remote.
    VlcHttp,
    /// Start and stop only.
    Launch,
}

impl Control {
    /// Whether play, pause and seek reach the player.
    pub fn is_full(self) -> bool {
        !matches!(self, Control::Launch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Option<Platform> {
        match std::env::consts::OS {
            "macos" => Some(Platform::Macos),
            "linux" => Some(Platform::Linux),
            "windows" => Some(Platform::Windows),
            _ => None,
        }
    }
}

/// One argument, or a group that is kept or dropped as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    One(String),
    Group(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSpec {
    pub description: String,
    /// Candidates: the first that exists is used.
    pub program: Vec<String>,
    pub args: Vec<Arg>,
    pub control: Control,
    #[serde(default)]
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    players: BTreeMap<String, PlayerSpec>,
}

/// Values for the placeholders of one cast. A missing key means "no value".
pub type Vars = HashMap<&'static str, String>;

impl PlayerSpec {
    /// A launch-only player from a command line such as
    /// `open -a "QuickTime Player" {url}`; without `{url}`, the URL goes last.
    pub fn from_command_line(line: &str) -> Result<PlayerSpec, String> {
        let mut words = split(line)?;
        if words.is_empty() {
            return Err("the player command is empty".into());
        }
        if !words.iter().any(|w| w.contains("{url}")) {
            words.push("{url}".into());
        }
        let program = words.remove(0);
        let spec = PlayerSpec {
            description: format!("command: {line}"),
            program: vec![program],
            args: words.into_iter().map(Arg::One).collect(),
            control: Control::Launch,
            platforms: vec![],
        };
        spec.check("command")?;
        Ok(spec)
    }

    pub fn runs_here(&self) -> bool {
        self.platforms.is_empty() || Platform::current().is_some_and(|p| self.platforms.contains(&p))
    }

    /// The program to start: the first candidate that exists (an absolute
    /// path on disk, or a bare name found on PATH).
    pub fn resolve_program(&self) -> Option<PathBuf> {
        self.program.iter().find_map(|p| which(p))
    }

    /// The argument list for one cast. An argument (or group) whose
    /// placeholder has no value is left out.
    pub fn expand_args(&self, vars: &Vars) -> Vec<String> {
        self.args
            .iter()
            .flat_map(|arg| {
                let words: &[String] = match arg {
                    Arg::One(w) => std::slice::from_ref(w),
                    Arg::Group(ws) => ws,
                };
                let filled: Option<Vec<String>> = words.iter().map(|w| fill(w, vars)).collect();
                filled.unwrap_or_default()
            })
            .collect()
    }

    /// Checks a spec when it is loaded, so a typo fails early with its name.
    fn check(&self, name: &str) -> Result<(), String> {
        if self.program.is_empty() {
            return Err(format!("player {name}: `program` is empty"));
        }
        let all: Vec<&String> = self
            .args
            .iter()
            .flat_map(|a| match a {
                Arg::One(w) => vec![w],
                Arg::Group(ws) => ws.iter().collect(),
            })
            .collect();
        for w in &all {
            for p in placeholders(w) {
                if !KNOWN.contains(&p) {
                    return Err(format!("player {name}: unknown placeholder {{{p}}} in {w:?}"));
                }
            }
        }
        if !all.iter().any(|w| w.contains("{url}")) {
            return Err(format!("player {name}: no argument contains {{url}}"));
        }
        let needs: &[&str] = match self.control {
            Control::MpvIpc => &["ipc"],
            Control::VlcHttp => &["port", "password"],
            Control::Launch => &[],
        };
        for n in needs {
            if !all.iter().any(|w| placeholders(w).any(|p| p == *n)) {
                return Err(format!("player {name}: control {:?} needs {{{n}}} in its args", self.control));
            }
        }
        Ok(())
    }
}

/// Splits a command line into words, keeping "double" or 'single' quoted parts together.
fn split(line: &str) -> Result<Vec<String>, String> {
    let (mut words, mut word, mut quote, mut in_word) = (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err(format!("unclosed quote in player command: {line}"));
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

const KNOWN: &[&str] = &["url", "title", "referer", "user_agent", "headers", "ipc", "port", "password"];

fn placeholders(word: &str) -> impl Iterator<Item = &str> {
    word.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
}

/// Fills every placeholder in `word`; `None` if one has no value.
fn fill(word: &str, vars: &Vars) -> Option<String> {
    let mut out = word.to_string();
    for p in placeholders(word) {
        let v = vars.get(p).filter(|v| !v.is_empty())?;
        out = out.replace(&format!("{{{p}}}"), v);
    }
    Some(out)
}

fn which(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return p.exists().then(|| p.to_path_buf());
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |e| dir.join(format!("{program}{e}"))))
        .find(|c| c.is_file())
}

/// All known players: the built-in list, then each file in `extra` (later
/// entries replace earlier ones with the same name).
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub players: BTreeMap<String, PlayerSpec>,
}

impl Registry {
    pub fn built_in() -> Registry {
        let mut r = Registry::default();
        r.merge_str(BUILT_IN, "built-in players.toml").expect("the built-in players.toml is valid");
        r
    }

    /// The built-in list plus the user's file, if present:
    /// `$CASTABLE_PLAYERS`, else `~/.config/castable/players.toml`.
    pub fn load() -> Result<Registry, String> {
        let mut r = Registry::built_in();
        if let Some(path) = user_file().filter(|p| p.exists()) {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            r.merge_str(&text, &path.display().to_string())?;
        }
        Ok(r)
    }

    pub fn merge_str(&mut self, toml_text: &str, source: &str) -> Result<(), String> {
        let file: File = toml::from_str(toml_text).map_err(|e| format!("{source}: {e}"))?;
        for (name, spec) in file.players {
            spec.check(&name).map_err(|e| format!("{source}: {e}"))?;
            self.players.insert(name, spec);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&PlayerSpec> {
        self.players.get(name)
    }
}

pub fn user_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CASTABLE_PLAYERS") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".config/castable/players.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_list_is_valid() {
        let r = Registry::built_in();
        for name in ["mpv", "iina", "vlc", "quicktime", "ffplay", "browser"] {
            assert!(r.get(name).is_some(), "missing {name}");
        }
        assert_eq!(r.get("iina").unwrap().control, Control::MpvIpc);
    }

    #[test]
    fn missing_values_drop_their_argument_or_group() {
        let spec = Registry::built_in().get("ffplay").unwrap().clone();
        let mut vars = Vars::new();
        vars.insert("url", "http://x/a.mp4".into());
        assert_eq!(spec.expand_args(&vars), ["-autoexit", "http://x/a.mp4"]);
        vars.insert("title", "Episode 1".into());
        assert_eq!(spec.expand_args(&vars), ["-autoexit", "-window_title", "Episode 1", "http://x/a.mp4"]);
    }

    #[test]
    fn a_user_file_adds_and_replaces_players() {
        let mut r = Registry::built_in();
        r.merge_str(
            r#"
            [players.mpv]
            description = "mpv, fullscreen"
            program = ["mpv"]
            args = ["--fs", "--input-ipc-server={ipc}", "{url}"]
            control = "mpv-ipc"

            [players.potplayer]
            description = "PotPlayer"
            program = ['C:\Program Files\DAUM\PotPlayer\PotPlayerMini64.exe']
            args = ["{url}"]
            control = "launch"
            platforms = ["windows"]
            "#,
            "test",
        )
        .unwrap();
        assert_eq!(r.get("mpv").unwrap().description, "mpv, fullscreen");
        assert!(r.get("potplayer").is_some());
    }

    #[test]
    fn a_command_line_becomes_a_launch_player() {
        let spec = PlayerSpec::from_command_line(r#"open -a "QuickTime Player""#).unwrap();
        assert_eq!(spec.program, ["open"]);
        let mut vars = Vars::new();
        vars.insert("url", "http://x/a.mp4".into());
        assert_eq!(spec.expand_args(&vars), ["-a", "QuickTime Player", "http://x/a.mp4"]);
        assert_eq!(split("ffplay  -autoexit {url}").unwrap(), ["ffplay", "-autoexit", "{url}"]);
        assert!(split(r#"open "unclosed"#).is_err());
    }

    #[test]
    fn mistakes_are_reported_with_the_player_name() {
        let mut r = Registry::default();
        let bad = |t: &str| Registry::default().merge_str(t, "f").unwrap_err();
        assert!(bad(r#"[players.x]
description = "x"
program = ["x"]
args = ["{urll}"]
control = "launch""#)
        .contains("unknown placeholder {urll}"));
        assert!(bad(r#"[players.x]
description = "x"
program = ["x"]
args = ["{url}"]
control = "mpv-ipc""#)
        .contains("needs {ipc}"));
        assert!(bad(r#"[players.x]
description = "x"
program = ["x"]
args = ["{url}"]
control = "telepathy""#)
        .contains("unknown variant"));
        assert!(r.merge_str("[players]", "empty").is_ok());
    }
}
