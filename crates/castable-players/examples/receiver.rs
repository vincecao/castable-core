//! A desktop test receiver: shows up in phone apps' cast lists and plays what
//! they send, in the player you choose.
//!
//! ```sh
//! cargo run -p castable-players --example receiver -- --players          # list players
//! cargo run -p castable-players --example receiver -- --name "My Mac" --player mpv
//! cargo run -p castable-players --example receiver -- --player iina
//! cargo run -p castable-players --example receiver -- --player 'open -a "QuickTime Player" {url}'
//! cargo run -p castable-players --example receiver                       # no player: log only
//! RUST_LOG=debug cargo run -p castable-players --example receiver        # every request and poll
//! cargo run -p castable-players --example receiver -- --log-json         # one JSON object per line
//! ```
//!
//! Players come from the built-in `players.toml` plus
//! `~/.config/castable/players.toml` (or `$CASTABLE_PLAYERS`).

use castable_net::{player::assume_loaded, Config, Handle, Player, PlayerCommand};
use castable_players::{External, PlayerSpec, Registry};
use tracing_subscriber::EnvFilter;

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

#[derive(Debug, Default)]
struct Args {
    name: Option<String>,
    player: Option<String>,
    list: bool,
    port: u16,
    log_json: bool,
}

fn args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--name" => a.name = it.next(),
            "--player" => a.player = it.next(),
            "--mpv" => a.player = Some("mpv".into()), // the older flag
            "--players" => a.list = true,
            "--port" => a.port = it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            "--log-json" => a.log_json = true,
            "-h" | "--help" => {
                println!("receiver [--name NAME] [--player NAME|'COMMAND {{url}}'] [--players] [--port HTTP_PORT] [--log-json]");
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

fn list(registry: &Registry) {
    println!(
        "Players (built in, plus {}):\n",
        castable_players::user_file().map_or("-".into(), |p| p.display().to_string())
    );
    for (name, spec) in &registry.players {
        let here = match (spec.runs_here(), spec.resolve_program()) {
            (false, _) => "not for this OS",
            (true, Some(_)) => "installed",
            (true, None) => "not installed",
        };
        println!("  {name:<10} {:<14} {}", here, spec.description);
    }
    println!("\nOr any command: --player 'COMMAND {{url}}' (launch only).");
}

fn pick(choice: Option<&str>, registry: &Registry, handle: Handle) -> Result<Backend, String> {
    let Some(choice) = choice.filter(|c| *c != "log" && *c != "none") else {
        return Ok(Backend::Log(handle));
    };
    let (name, spec) = match registry.get(choice) {
        Some(spec) => (choice, spec.clone()),
        None if choice.contains(' ') || choice.contains("{url}") => ("command", PlayerSpec::from_command_line(choice)?),
        None => return Err(format!("no player named {choice}; see --players")),
    };
    Ok(Backend::External(External::new(name, spec, handle)?))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = args();
    // The filter runs before a line is built: a disabled level costs one atomic check.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if args.log_json {
        tracing_subscriber::fmt().json().flatten_event(true).with_current_span(false).with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().compact().with_env_filter(filter).init();
    }
    let registry = Registry::load().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    if args.list {
        list(&registry);
        return Ok(());
    }

    let host =
        std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "desktop".into());
    let name = args.name.unwrap_or_else(|| format!("Castable ({host})"));
    let mut config = Config::named(&name);
    config.http.set_port(args.port);
    let rx = castable_net::start(config).await?;

    let backend = pick(args.player.as_deref(), &registry, rx.handle.clone()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    let control = match &backend {
        Backend::External(p) => {
            if p.control().is_full() {
                "full"
            } else {
                "launch only"
            }
        }
        Backend::Log(_) => "none (log only)",
    };
    tracing::info!(target: "player", player = args.player.as_deref().unwrap_or("log"), control, "player ready");

    let ip = castable_net::local_ip().map_or("<this computer's IP>".into(), |ip| ip.to_string());
    println!(
        "\n  \"{name}\" is ready. Open a cast app on a phone on the same Wi-Fi.\n  Description: http://{ip}:{}/desc.xml\n",
        rx.http_addr.port()
    );

    castable_net::run(rx, backend, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
    Ok(())
}
