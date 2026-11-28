//! `castable-receiver`: a cast receiver for this computer. Phones find it on
//! the network and play what they cast in the player you choose.
//!
//! ```sh
//! castable-receiver                                   # settings from the setup wizard
//! castable-receiver --name "My Mac" --player mpv      # flags override settings
//! castable-receiver --player 'open -a "QuickTime Player" {url}'
//! castable-receiver --players [--json]                # which players are available
//! RUST_LOG=debug castable-receiver                    # every request and poll
//! castable-receiver --log json                        # one JSON object per line
//! castable-receiver --report                          # a report to attach to an issue
//! castable-receiver --control stdin                   # player keys, one per line ("seek-back"),
//!                                                     # or "rename NAME" (saved to the settings)
//! ```
//!
//! Every run also writes a detailed log (player output included) to
//! `logs/receiver.log` in the config folder (`--log-file PATH` to move it,
//! `--no-log-file` to skip it). Errors carry a code such as `E403` from
//! `castable_core::ErrorCode`, the same for every player.
//!
//! Settings: `config.toml` in Castable's config folder (`--config PATH` or
//! `$CASTABLE_CONFIG` to change). Players: the built-in `players.toml` plus
//! the user's own in the same folder.

use castable_net::{player::assume_loaded, CastError, Config, ErrorCode, Handle, Player, PlayerCommand};
use castable_players::{report, settings_file, Control, External, LogFormat, PlayerSpec, Registry, Settings};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

const USAGE: &str = "castable-receiver [--name NAME] [--player NAME|'COMMAND {url}'|log] [--port PORT]
                  [--log pretty|json] [--log-file PATH | --no-log-file] [--config PATH]
                  [--players [--json]] [--print-config-path] [--print-default-name]
                  [--report [PATH]] [--control stdin]";

/// The chosen player: one from the registry, or none (log only).
enum Backend {
    // Boxed: a player is much larger than a handle (clippy::large_enum_variant on Windows).
    External(Box<External>),
    Log(Handle),
}

impl Player for Backend {
    async fn apply(&mut self, command: PlayerCommand) {
        match self {
            Backend::External(p) => p.apply(command).await,
            // No player: report every load as done, so senders see the receiver work.
            Backend::Log(handle) => {
                if let PlayerCommand::Load { gen, .. } = command {
                    assume_loaded(handle, gen);
                }
            }
        }
    }

    async fn close(&mut self) {
        if let Backend::External(p) = self {
            p.close().await;
        }
    }
}

/// What the command line asks for, on top of the settings file.
#[derive(Debug, Default)]
struct Flags {
    settings: Settings,
    config: Option<PathBuf>,
    list: bool,
    json: bool,
    print_config_path: bool,
    /// `--print-default-name`: the name a fresh install shows (the setup wizard's suggestion).
    print_default_name: bool,
    /// Where this run's detailed log goes; `None` with `--no-log-file`.
    log_file: Option<PathBuf>,
    /// `--report [PATH]`: write a report and exit.
    report: Option<Option<PathBuf>>,
    /// `--control stdin`: read player keys (`castable_core::keys`), one name per line.
    control_stdin: bool,
}

/// Prints the error with its code ("E703 config/player_unknown: …") and exits.
fn fail(error: CastError) -> ! {
    tracing::error!(code = %error.code.id(), error = error.code.name(), detail = error.detail.as_deref().unwrap_or(""), "{}", error.code.summary());
    eprintln!("castable-receiver: {error}");
    std::process::exit(2)
}

fn bad_arg(detail: impl Into<String>) -> CastError {
    CastError::with_detail(ErrorCode::BadArgument, detail)
}

fn flags() -> Flags {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let config = argv.iter().position(|a| a == "--config").and_then(|i| argv.get(i + 1)).map(PathBuf::from);
    let path = config.clone().or_else(settings_file);
    let settings = path
        .as_deref()
        .map(Settings::load)
        .transpose()
        .unwrap_or_else(|e| fail(CastError::with_detail(ErrorCode::SettingsInvalid, e)))
        .unwrap_or_default();
    let log_file = report::log_dir().map(|d| d.join("receiver.log"));
    let mut f = Flags { settings, config, log_file, ..Flags::default() };
    let mut it = argv.into_iter().peekable();
    while let Some(arg) = it.next() {
        if arg == "--report" {
            f.report = Some(it.next_if(|v| !v.starts_with("--")).map(PathBuf::from));
            continue;
        }
        let mut value = || it.next().unwrap_or_else(|| fail(bad_arg(format!("{arg} needs a value"))));
        match arg.as_str() {
            "--name" => f.settings.name = Some(value()),
            "--player" => f.settings.player = Some(value()),
            "--mpv" => f.settings.player = Some("mpv".into()), // the older flag
            "--port" => {
                f.settings.port = Some(value().parse().unwrap_or_else(|_| fail(bad_arg("--port needs a number"))))
            }
            "--log" => {
                f.settings.log = match value().as_str() {
                    "json" => LogFormat::Json,
                    "pretty" => LogFormat::Pretty,
                    other => fail(bad_arg(format!("--log takes pretty or json, not {other}"))),
                }
            }
            "--log-json" => f.settings.log = LogFormat::Json, // the older flag
            "--log-file" => f.log_file = Some(PathBuf::from(value())),
            "--no-log-file" => f.log_file = None,
            "--config" => {
                value();
            }
            "--players" => f.list = true,
            // For the setup wizard: where settings live on this OS, so it never guesses.
            "--print-config-path" => f.print_config_path = true,
            "--print-default-name" => f.print_default_name = true,
            "--json" => f.json = true,
            "--control" => match value().as_str() {
                "stdin" => f.control_stdin = true,
                other => fail(bad_arg(format!("--control takes stdin, not {other}"))),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => fail(bad_arg(format!("unknown argument {other}\n{USAGE}"))),
        }
    }
    f
}

/// One line per player, or JSON for the setup wizard.
fn list(registry: &Registry, json: bool) {
    let rows: Vec<serde_json::Value> = registry
        .players
        .iter()
        .map(|(name, spec)| {
            serde_json::json!({
                "name": name,
                "description": spec.description,
                "control": if spec.control.is_full() { "full" } else { "launch" },
                "availableHere": spec.runs_here(),
                "installed": spec.runs_here() && spec.resolve_program().is_some(),
            })
        })
        .collect();
    if json {
        println!("{}", serde_json::Value::Array(rows));
        return;
    }
    let here = castable_players::user_file().map_or_else(|| "-".into(), |p| p.display().to_string());
    println!("Players (built in, plus {here}):\n");
    for r in &rows {
        let status = match (r["availableHere"].as_bool(), r["installed"].as_bool()) {
            (Some(false), _) => "not for this OS",
            (_, Some(true)) => "installed",
            _ => "not installed",
        };
        println!(
            "  {:<10} {:<15} {}",
            r["name"].as_str().unwrap_or(""),
            status,
            r["description"].as_str().unwrap_or("")
        );
    }
    println!("\nOr any command: --player 'COMMAND {{url}}' (launch only).");
}

fn pick(
    choice: Option<&str>,
    registry: &Registry,
    handle: Handle,
    font: Option<String>,
) -> Result<(Backend, &'static str), CastError> {
    let Some(choice) = choice.filter(|c| !matches!(*c, "log" | "none" | "")) else {
        return Ok((Backend::Log(handle), "none"));
    };
    let (name, spec) = match registry.get(choice) {
        Some(spec) => (choice, spec.clone()),
        None if choice.contains(' ') || choice.contains("{url}") => {
            ("command", PlayerSpec::from_command_line(choice).map_err(bad_arg)?)
        }
        None => {
            return Err(CastError::with_detail(ErrorCode::PlayerUnknown, format!("{choice}; see --players")));
        }
    };
    // As `--players --json` says it: "full", "launch", or "none" (no player, log only).
    let control = if spec.control == Control::Launch { "launch" } else { "full" };
    Ok((Backend::External(Box::new(External::new(name, spec, handle)?.with_font(font))), control))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Flags {
        settings,
        config,
        list: show_list,
        json,
        print_config_path,
        print_default_name,
        log_file,
        report: report_to,
        control_stdin,
    } = flags();
    if print_default_name {
        println!("{}", castable_net::profile::default_name());
        return Ok(());
    }
    if print_config_path {
        println!("{}", config.or_else(settings_file).map(|p| p.display().to_string()).unwrap_or_default());
        return Ok(());
    }
    let registry = Registry::load().unwrap_or_else(|e| fail(CastError::with_detail(ErrorCode::PlayersFileInvalid, e)));
    if let Some(to) = report_to {
        let settings_path = config.clone().or_else(settings_file);
        let log = log_file.clone().unwrap_or_else(|| PathBuf::from("receiver.log"));
        let md = report::build(&settings, settings_path.as_deref(), &registry, &log);
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let to = to.unwrap_or_else(|| PathBuf::from(format!("castable-report-{secs}.md")));
        std::fs::write(&to, md)
            .unwrap_or_else(|e| fail(CastError::with_detail(ErrorCode::Io, format!("{}: {e}", to.display()))));
        println!("{}", to.display());
        return Ok(());
    }
    if show_list {
        list(&registry, json);
        return Ok(());
    }

    // Filters run before a line is built: a disabled level costs one atomic check.
    // The terminal gets what happened (info); the file also gets every request
    // and what the player printed (debug), for reports.
    let console_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let console = match settings.log {
        LogFormat::Json => tracing_subscriber::fmt::layer().json().flatten_event(true).with_current_span(false).boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().compact().boxed(),
    };
    let (file, file_error) = match log_file.as_deref().map(report::open_log) {
        Some(Ok(f)) => (Some(f), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    let file_layer = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .with_writer(Mutex::new(f))
            .with_filter(EnvFilter::new(
                "info,castable_net=debug,castable_players=debug,player=debug,castable_receiver=debug",
            ))
    });
    tracing_subscriber::registry().with(console.with_filter(console_filter)).with(file_layer).init();
    if let (Some(e), Some(path)) = (file_error, log_file.as_deref()) {
        tracing::warn!(code = %ErrorCode::Io.id(), error = ErrorCode::Io.name(), detail = %e, path = %path.display(), "can't write the log file");
    }

    let name = settings.name.clone().unwrap_or_else(castable_net::profile::default_name);
    let settings_path = config.clone().or_else(settings_file);
    let mut net = Config::named(&name);
    // One id per install, beside the settings; older versions' name-derived id seeds it.
    net.uuid = settings_path
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|dir| castable_net::profile::device_id(dir, Some(castable_net::profile::legacy_id(&name))));
    net.http.set_port(settings.port.unwrap_or(castable_net::PREFERRED_PORT));
    let rx = castable_net::start(net).await.unwrap_or_else(|e| {
        let code = match e.kind() {
            std::io::ErrorKind::AddrInUse => ErrorCode::PortInUse,
            std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::NetworkUnreachable => ErrorCode::NoNetwork,
            _ => ErrorCode::Io,
        };
        fail(CastError::with_detail(code, e.to_string()))
    });

    let (backend, control) =
        pick(settings.player.as_deref(), &registry, rx.handle.clone(), settings.font()).unwrap_or_else(|e| fail(e));
    let ip = castable_net::local_ip().map_or_else(|| "?".into(), |ip| ip.to_string());
    let settings_shown = settings_path.as_deref().map(|p| p.display().to_string()).unwrap_or_default();
    tracing::info!(
        name = %name,
        player = settings.player.as_deref().unwrap_or("log"),
        control,
        url = %format!("http://{ip}:{}/desc.xml", rx.http_addr.port()),
        settings = %settings_shown,
        log_file = %log_file.as_deref().map(|p| p.display().to_string()).unwrap_or_default(),
        "ready"
    );
    if settings.log == LogFormat::Pretty {
        println!("\n  \"{name}\" is ready. Open a cast app on a phone on the same Wi-Fi and pick it.\n");
    }

    // Ctrl-C, or with `--control stdin` the front end closing its input (how
    // the CLI stops it on Windows, where it can't send Ctrl-C): either way the
    // receiver says goodbye on the network and closes its player.
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<()>();
    if control_stdin {
        let handle = rx.handle.clone();
        tokio::spawn(async move {
            control_from_stdin(handle, settings_path).await;
            let _ = closed_tx.send(());
        });
    }
    castable_net::run(rx, backend, async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            Ok(()) = closed_rx => tracing::info!("control input closed: stopping"),
        }
    })
    .await;
    Ok(())
}

/// `--control stdin`, for a front end such as the CLI's status screen. Each
/// line is a player key (`play-pause`, `seek-back`, `volume-up`, ... see
/// `castable_core::keys`), acted on with what's shown now, or `rename NAME`:
/// renamed live, saved to the settings, and logged as `renamed` (or, refused,
/// as `rename refused` with the reason). Returns when the input closes.
async fn control_from_stdin(handle: Handle, settings: Option<PathBuf>) {
    use tokio::io::AsyncBufReadExt;
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(name) = line.strip_prefix("rename ") {
            rename(&handle, settings.as_deref(), name);
            continue;
        }
        match castable_net::PlayerKey::parse(&line) {
            Some(key) => handle.key(key),
            None => tracing::warn!(line = %line.trim(), "not a player key"),
        }
    }
}

fn rename(handle: &Handle, settings: Option<&std::path::Path>, name: &str) {
    match handle.rename(name) {
        Ok(name) => {
            if let Some(path) = settings {
                if let Err(e) = Settings::save_name(path, &name) {
                    tracing::warn!(code = %ErrorCode::Io.id(), error = ErrorCode::Io.name(), detail = %e, "can't save the new name; it lasts until the receiver stops");
                }
            }
            tracing::info!(name = %name, "renamed");
        }
        Err(reason) => tracing::warn!(reason = ?reason, detail = %reason, "rename refused"),
    }
}
