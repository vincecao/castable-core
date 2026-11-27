//! A desktop test receiver: shows up in phone apps' cast lists and plays what
//! they send. With `--mpv` it plays in mpv (controlled over mpv's IPC socket);
//! without it, it only logs what a player would do.
//!
//! ```sh
//! cargo run -p castable-net --example receiver -- --name "My Mac" --mpv
//! RUST_LOG=debug cargo run -p castable-net --example receiver   # also every request and poll
//! cargo run -p castable-net --example receiver -- --log-json    # one JSON object per line
//!
//! Log levels: info = what happened (casts, commands, state changes);
//! debug = every HTTP request, SSDP search and status poll; trace = playback progress.
//! ```

use castable_net::{Command, Config, Gen, PlayerCommand, PlayerEvent};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Default)]
struct Args {
    name: Option<String>,
    mpv: bool,
    port: u16,
    log_json: bool,
}

fn args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--name" => a.name = it.next(),
            "--mpv" => a.mpv = true,
            "--port" => a.port = it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            "--log-json" => a.log_json = true,
            "-h" | "--help" => {
                println!("receiver [--name NAME] [--mpv] [--port HTTP_PORT] [--log-json]");
                std::process::exit(0);
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
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
                Some(_) => tracing::debug!(target: "player", "volume and mute are not applied in this demo"),
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
    /// Commands for the running mpv. They queue until its IPC socket is up,
    /// so a Play sent while mpv is still starting is never lost.
    ipc: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl Player {
    fn new(mpv: bool, handle: castable_net::Handle) -> Self {
        Self { mpv, handle, stop_mpv: None, ipc: None }
    }

    async fn run(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::Load { gen, media } => {
                tracing::debug!(target: "player", gen = gen.0, backend = if self.mpv { "mpv" } else { "none" }, "load");
                self.kill().await;
                if self.mpv {
                    self.spawn_mpv(gen, &media);
                } else {
                    // No real player: pretend the load finished.
                    self.handle.player_event(PlayerEvent::Loaded { gen, duration: None });
                }
            }
            PlayerCommand::Play => {
                self.mpv_ipc(r#"{"command":["set_property","pause",false]}"#);
            }
            PlayerCommand::Pause => {
                self.mpv_ipc(r#"{"command":["set_property","pause",true]}"#);
            }
            PlayerCommand::Stop => {
                self.mpv_ipc(r#"{"command":["set_property","pause",true]}"#);
                self.mpv_ipc(r#"{"command":["seek",0,"absolute"]}"#);
            }
            PlayerCommand::Seek { to } => {
                self.mpv_ipc(&format!(r#"{{"command":["seek",{},"absolute"]}}"#, to.as_secs_f64()));
            }
        }
    }

    fn spawn_mpv(&mut self, gen: Gen, media: &castable_net::MediaDescriptor) {
        // One socket per load, so a late connection never reaches the next mpv.
        let socket = std::env::temp_dir().join(format!("castable-mpv-{}-{}.sock", std::process::id(), gen.0));
        let mut cmd = tokio::process::Command::new("mpv");
        cmd.arg("--pause").arg("--force-window=immediate").arg("--keep-open=no");
        cmd.arg(format!("--input-ipc-server={}", socket.display()));
        if !media.headers.is_empty() {
            let fields: Vec<String> = media.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            cmd.arg(format!("--http-header-fields={}", fields.join(",")));
        }
        cmd.arg(&media.url).kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(target: "player", error = %e, "can't start mpv; is it installed? (brew install mpv)");
                self.handle.player_event(PlayerEvent::LoadFailed { gen, reason: e.to_string() });
                return;
            }
        };
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (ipc_tx, ipc_rx) = tokio::sync::mpsc::unbounded_channel();
        self.stop_mpv = Some(stop_tx);
        self.ipc = Some(ipc_tx);
        tokio::spawn(mpv::session(gen, socket, ipc_rx, self.handle.clone()));
        // If mpv dies before reporting anything, the load failed. Events for an
        // older load are ignored by the core, so racing a kill is harmless.
        let handle = self.handle.clone();
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
        self.ipc = None;
        if let Some(stop) = self.stop_mpv.take() {
            let _ = stop.send(());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn mpv_ipc(&self, json: &str) {
        if let Some(tx) = &self.ipc {
            let _ = tx.send(json.to_string());
        }
    }
}

/// mpv's JSON IPC: reports when the file is really loaded (with its
/// duration), the position once a second, and how playback ended.
mod mpv {
    use castable_net::{Gen, Handle, PlayerEvent};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::UnboundedReceiver;

    #[cfg(unix)]
    pub async fn session(gen: Gen, socket: PathBuf, mut commands: UnboundedReceiver<String>, handle: Handle) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        // mpv creates the socket a moment after it starts (longer on a slow stream).
        let started = Instant::now();
        let stream = loop {
            match tokio::net::UnixStream::connect(&socket).await {
                Ok(s) => break s,
                Err(_) if started.elapsed() < Duration::from_secs(20) => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(e) => {
                    handle.player_event(PlayerEvent::LoadFailed { gen, reason: format!("mpv didn't start: {e}") });
                    return;
                }
            }
        };
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        for watch in
            [r#"{"command":["observe_property",1,"duration"]}"#, r#"{"command":["observe_property",2,"time-pos"]}"#]
        {
            let _ = write.write_all(format!("{watch}\n").as_bytes()).await;
        }
        let (mut loaded, mut duration, mut last_progress) = (false, None, Instant::now());
        loop {
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    Some(json) => { let _ = write.write_all(format!("{json}\n").as_bytes()).await; }
                    None => return, // a new load replaced this one
                },
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { return };
                    let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                    let secs = |v: &serde_json::Value| v.as_f64().filter(|s| s.is_finite() && *s >= 0.0).map(Duration::from_secs_f64);
                    match (msg["event"].as_str(), msg["name"].as_str()) {
                        (Some("file-loaded"), _) => {
                            loaded = true;
                            handle.player_event(PlayerEvent::Loaded { gen, duration });
                        }
                        (Some("property-change"), Some("duration")) => duration = secs(&msg["data"]),
                        (Some("property-change"), Some("time-pos")) if loaded && last_progress.elapsed() >= Duration::from_secs(1) => {
                            if let Some(position) = secs(&msg["data"]) {
                                last_progress = Instant::now();
                                handle.player_event(PlayerEvent::Progress { gen, position, duration });
                            }
                        }
                        (Some("end-file"), _) => {
                            let event = match msg["reason"].as_str() {
                                Some("eof") => PlayerEvent::Ended { gen },
                                Some("error") => PlayerEvent::LoadFailed {
                                    gen,
                                    reason: msg["file_error"].as_str().unwrap_or("mpv couldn't play it").to_string(),
                                },
                                _ => continue, // stopped or replaced by us
                            };
                            handle.player_event(event);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Without Unix sockets (Windows) there is no IPC here: assume the load
    /// worked after a moment, and drop control commands.
    #[cfg(not(unix))]
    pub async fn session(gen: Gen, _socket: PathBuf, mut commands: UnboundedReceiver<String>, handle: Handle) {
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.player_event(PlayerEvent::Loaded { gen, duration: None });
        while commands.recv().await.is_some() {}
    }
}
