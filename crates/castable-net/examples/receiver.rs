//! A desktop test receiver: shows up in phone apps' cast lists and plays what
//! they send. With `--mpv` it plays in mpv (controlled over mpv's IPC socket);
//! without it, it only logs what a player would do.
//!
//! ```sh
//! cargo run -p castable-net --example receiver -- --name "My Mac" --mpv
//! RUST_LOG=debug cargo run -p castable-net --example receiver   # log every request
//! ```

use castable_net::{Command, Config, Gen, PlayerCommand, PlayerEvent};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Default)]
struct Args {
    name: Option<String>,
    mpv: bool,
    port: u16,
}

fn args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--name" => a.name = it.next(),
            "--mpv" => a.mpv = true,
            "--port" => a.port = it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            "-h" | "--help" => {
                println!("receiver [--name NAME] [--mpv] [--port HTTP_PORT]");
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let args = args();
    let host =
        std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).unwrap_or_else(|_| "desktop".into());
    let name = args.name.unwrap_or_else(|| format!("Castable ({host})"));
    let mut config = Config::named(&name);
    config.http.set_port(args.port);
    let mut rx = castable_net::start(config).await?;
    let ip = castable_net::ssdp::local_ip_for(castable_net::ssdp::MULTICAST.into())
        .map_or("<this computer's IP>".into(), |ip| ip.to_string());
    println!("\n  \"{name}\" is ready. Open a cast app on a phone on the same Wi-Fi.\n  Description: http://{ip}:{}/desc.xml\n", rx.http_addr.port());

    let handle = rx.handle.clone();
    let mut player = Player::new(args.mpv, handle.clone());
    loop {
        tokio::select! {
            cmd = rx.commands.recv() => match cmd {
                Some(Command::Player(c)) => player.run(c).await,
                Some(other) => tracing::info!(?other, "mixer command (not applied in this demo)"),
                None => break,
            },
            _ = tokio::signal::ctrl_c() => {
                player.kill().await;
                handle.shutdown().await;
                break;
            }
        }
    }
    Ok(())
}

/// The platform side: turns commands into a real player, reports events.
struct Player {
    mpv: bool,
    handle: castable_net::Handle,
    /// Stops the running mpv; dropping it does too.
    stop_mpv: Option<tokio::sync::oneshot::Sender<()>>,
    #[cfg(unix)]
    ipc: std::path::PathBuf,
}

impl Player {
    fn new(mpv: bool, handle: castable_net::Handle) -> Self {
        Self {
            mpv,
            handle,
            stop_mpv: None,
            #[cfg(unix)]
            ipc: std::env::temp_dir().join(format!("cast-mpv-{}.sock", std::process::id())),
        }
    }

    async fn run(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::Load { gen, media } => {
                println!("▶ load #{} {}", gen.0, media.title.as_deref().unwrap_or(&media.url));
                self.kill().await;
                if self.mpv {
                    self.spawn_mpv(gen, &media);
                } else {
                    // No real player: pretend the load finished.
                    self.handle.player_event(PlayerEvent::Loaded { gen, duration: None });
                }
            }
            PlayerCommand::Play => {
                println!("▶ play");
                self.mpv_ipc(r#"{"command":["set_property","pause",false]}"#).await;
            }
            PlayerCommand::Pause => {
                println!("⏸ pause");
                self.mpv_ipc(r#"{"command":["set_property","pause",true]}"#).await;
            }
            PlayerCommand::Stop => {
                println!("⏹ stop");
                self.mpv_ipc(r#"{"command":["set_property","pause",true]}"#).await;
                self.mpv_ipc(r#"{"command":["seek",0,"absolute"]}"#).await;
            }
            PlayerCommand::Seek { to } => {
                println!("⏩ seek {}s", to.as_secs());
                self.mpv_ipc(&format!(r#"{{"command":["seek",{},"absolute"]}}"#, to.as_secs_f64())).await;
            }
        }
    }

    fn spawn_mpv(&mut self, gen: Gen, media: &castable_net::MediaDescriptor) {
        let mut cmd = tokio::process::Command::new("mpv");
        cmd.arg("--pause").arg("--force-window=immediate").arg("--keep-open=no");
        #[cfg(unix)]
        cmd.arg(format!("--input-ipc-server={}", self.ipc.display()));
        if !media.headers.is_empty() {
            let fields: Vec<String> = media.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            cmd.arg(format!("--http-header-fields={}", fields.join(",")));
        }
        cmd.arg(&media.url).kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("can't start mpv ({e}); is it installed? (brew install mpv)");
                self.handle.player_event(PlayerEvent::LoadFailed { gen, reason: e.to_string() });
                return;
            }
        };
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        self.stop_mpv = Some(stop_tx);
        let handle = self.handle.clone();
        // mpv has no "loaded" signal without polling; give it a moment to open the stream.
        let loaded = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            loaded.player_event(PlayerEvent::Loaded { gen, duration: None });
        });
        // When mpv exits on its own the video ended (or failed). Events for an
        // older load are ignored by the session, so racing a kill is harmless.
        tokio::spawn(async move {
            tokio::select! {
                status = child.wait() => match status {
                    Ok(s) if s.success() => handle.player_event(PlayerEvent::Ended { gen }),
                    Ok(s) => handle.player_event(PlayerEvent::LoadFailed { gen, reason: format!("mpv exited with {s}") }),
                    Err(e) => handle.player_event(PlayerEvent::LoadFailed { gen, reason: e.to_string() }),
                },
                _ = stop_rx => { let _ = child.kill().await; }
            }
        });
    }

    async fn kill(&mut self) {
        if let Some(stop) = self.stop_mpv.take() {
            let _ = stop.send(());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[cfg(unix)]
    async fn mpv_ipc(&self, json: &str) {
        use tokio::io::AsyncWriteExt;
        if !self.mpv {
            return;
        }
        match tokio::net::UnixStream::connect(&self.ipc).await {
            Ok(mut s) => {
                let _ = s.write_all(format!("{json}\n").as_bytes()).await;
            }
            Err(e) => tracing::warn!(error = %e, "mpv IPC unavailable"),
        }
    }

    #[cfg(not(unix))]
    async fn mpv_ipc(&self, _json: &str) {}
}
