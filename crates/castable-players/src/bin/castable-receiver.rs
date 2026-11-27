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
//! ```
//!
//! Settings: `config.toml` in Castable's config folder (`--config PATH` or
//! `$CASTABLE_CONFIG` to change). Players: the built-in `players.toml` plus
//! the user's own in the same folder.

use castable_net::{player::assume_loaded, Config, Handle, Player, PlayerCommand};
use castable_players::{settings_file, Control, External, LogFormat, PlayerSpec, Registry, Settings};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "castable-receiver [--name NAME] [--player NAME|'COMMAND {url}'|log] [--port PORT]
                  [--log pretty|json] [--config PATH] [--players [--json]] [--print-config-path]";

/// The chosen player: one from the registry, or none (log only).
enum Backend {
    External(External),
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
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("castable-receiver: {message}");
    std::process::exit(2)
}

fn flags() -> Flags {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let config = argv.iter().position(|a| a == "--config").and_then(|i| argv.get(i + 1)).map(PathBuf::from);
    let path = config.clone().or_else(settings_file);
    let settings = path.as_deref().map(Settings::load).transpose().unwrap_or_else(|e| fail(e)).unwrap_or_default();
    let mut f = Flags { settings, config, ..Flags::default() };
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| fail(format!("{arg} needs a value")));
        match arg.as_str() {
            "--name" => f.settings.name = Some(value()),
            "--player" => f.settings.player = Some(value()),
            "--mpv" => f.settings.player = Some("mpv".into()), // the older flag
            "--port" => f.settings.port = value().parse().unwrap_or_else(|_| fail("--port needs a number")),
            "--log" => {
                f.settings.log = match value().as_str() {
                    "json" => LogFormat::Json,
                    "pretty" => LogFormat::Pretty,
                    other => fail(format!("--log takes pretty or json, not {other}")),
                }
            }
            "--log-json" => f.settings.log = LogFormat::Json, // the older flag
            "--config" => {
                value();
            }
            "--players" => f.list = true,
            // For the setup wizard: where settings live on this OS, so it never guesses.
            "--print-config-path" => f.print_config_path = true,
            "--json" => f.json = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => fail(format!("unknown argument {other}\n{USAGE}")),
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

fn pick(choice: Option<&str>, registry: &Registry, handle: Handle) -> Result<(Backend, &'static str), String> {
    let Some(choice) = choice.filter(|c| !matches!(*c, "log" | "none" | "")) else {
        return Ok((Backend::Log(handle), "none (log only)"));
    };
    let (name, spec) = match registry.get(choice) {
        Some(spec) => (choice, spec.clone()),
        None if choice.contains(' ') || choice.contains("{url}") => ("command", PlayerSpec::from_command_line(choice)?),
        None => return Err(format!("no player named {choice}; see --players")),
    };
    let control = if spec.control == Control::Launch { "launch only" } else { "full" };
    Ok((Backend::External(External::new(name, spec, handle)?), control))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Flags { settings, config, list: show_list, json, print_config_path } = flags();
    if print_config_path {
        println!("{}", config.or_else(settings_file).map(|p| p.display().to_string()).unwrap_or_default());
        return Ok(());
    }
    let registry = Registry::load().unwrap_or_else(|e| fail(e));
    if show_list {
        list(&registry, json);
        return Ok(());
    }

    // The filter runs before a line is built: a disabled level costs one atomic check.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match settings.log {
        LogFormat::Json => {
            tracing_subscriber::fmt().json().flatten_event(true).with_current_span(false).with_env_filter(filter).init()
        }
        LogFormat::Pretty => tracing_subscriber::fmt().compact().with_env_filter(filter).init(),
    }

    let host =
        std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "this computer".into());
    let name = settings.name.clone().unwrap_or_else(|| format!("Castable ({host})"));
    let mut net = Config::named(&name);
    net.http.set_port(settings.port);
    let rx = castable_net::start(net).await?;

    let (backend, control) = pick(settings.player.as_deref(), &registry, rx.handle.clone()).unwrap_or_else(|e| fail(e));
    let ip = castable_net::local_ip().map_or_else(|| "?".into(), |ip| ip.to_string());
    let settings_path = config.or_else(settings_file).map(|p| p.display().to_string()).unwrap_or_default();
    tracing::info!(
        name = %name,
        player = settings.player.as_deref().unwrap_or("log"),
        control,
        url = %format!("http://{ip}:{}/desc.xml", rx.http_addr.port()),
        settings = %settings_path,
        "ready"
    );
    if settings.log == LogFormat::Pretty {
        println!("\n  \"{name}\" is ready. Open a cast app on a phone on the same Wi-Fi and pick it.\n");
    }

    castable_net::run(rx, backend, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
    Ok(())
}
