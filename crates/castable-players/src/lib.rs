//! `castable-players`: external players for the Castable receiver.
//!
//! Players are data: each is an entry in `players.toml` (built in) or the
//! user's own `~/.config/castable/players.toml`, naming a program, its
//! arguments, and one of a few control protocols. Adding a player that
//! speaks a known protocol needs no code.
//!
//! ```no_run
//! # async fn demo(rx: castable_net::Receiver) -> Result<(), String> {
//! let registry = castable_players::Registry::load()?;
//! let spec = registry.get("mpv").ok_or("no mpv entry")?.clone();
//! let player = castable_players::External::new("mpv", spec, rx.handle.clone())?;
//! castable_net::run(rx, player, async { let _ = tokio::signal::ctrl_c().await; }).await;
//! # Ok(()) }
//! ```

pub mod external;
mod mpv;
mod process;
pub mod spec;
mod vlc;

pub use external::External;
pub use spec::{user_file, Control, PlayerSpec, Registry};
