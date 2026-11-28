//! What a receiver keeps between runs, the same in every app (desktop, phone,
//! CLI), so they behave alike for the people using them:
//!
//! - One id per install, saved in the app's folder. Phones list a receiver by
//!   this id, so a renamed receiver stays the same device to them.
//! - A fresh install is called "Castable (<computer name>)": unique on a
//!   network with several receivers, and it says what it is.
//! - Names follow `castable_core::device::friendly_name` (1–64 characters).
//!
//! Where an app saves the chosen name is its own business (the CLI keeps it
//! in `config.toml`); `--name`/`CASTABLE_NAME` style overrides are per run.

use std::fs;
use std::path::Path;

const ID_FILE: &str = "device-id";

/// This install's id, kept in `dir/device-id`. A new install saves `seed` if
/// given (see [`legacy_id`]), else a random id. If it can't be saved, it's
/// used for this run only (and the next run gets a new one).
pub fn device_id(dir: &Path, seed: Option<String>) -> String {
    let file = dir.join(ID_FILE);
    if let Ok(id) = fs::read_to_string(&file) {
        let id = id.trim();
        if !id.is_empty() {
            return id.to_owned();
        }
    }
    let id = seed.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Err(e) = fs::create_dir_all(dir).and_then(|()| fs::write(&file, &id)) {
        tracing::warn!(error = %e, path = %file.display(), "can't save the device id; it changes on the next start");
    }
    id
}

/// The id versions before saved ids derived from this computer and the name.
/// Seeding [`device_id`] with it keeps an existing receiver the same device to
/// the phones that already know it.
pub fn legacy_id(name: &str) -> String {
    let host = std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_default();
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, format!("{host}/{name}").as_bytes()).to_string()
}

/// The name a fresh install shows to phones.
pub fn default_name() -> String {
    match computer_name() {
        Some(computer) => format!("Castable ({computer})"),
        None => "Castable".to_owned(),
    }
}

/// The computer's name as its owner knows it: macOS's Computer Name
/// ("Ada's MacBook Pro"), else the host name without `.local`.
pub fn computer_name() -> Option<String> {
    let macos = cfg!(target_os = "macos")
        .then(|| std::process::Command::new("scutil").args(["--get", "ComputerName"]).output().ok())
        .flatten()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
    let host = || {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok()
            .or_else(|| fs::read_to_string("/proc/sys/kernel/hostname").ok())
    };
    let name = macos.or_else(host)?;
    let name = name.trim().trim_end_matches(".local").trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_saved_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("castable-profile-{}", uuid::Uuid::new_v4()));
        let first = device_id(&dir, Some("seed-id".into()));
        assert_eq!(first, "seed-id");
        assert_eq!(device_id(&dir, Some("other".into())), "seed-id");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_legacy_id_depends_only_on_computer_and_name() {
        assert_eq!(legacy_id("Living room"), legacy_id("Living room"));
        assert_ne!(legacy_id("Living room"), legacy_id("Bedroom"));
    }

    #[test]
    fn the_default_name_says_castable() {
        assert!(default_name().starts_with("Castable"));
    }
}
