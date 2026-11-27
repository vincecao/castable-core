# castable-core

The core of [Castable](https://github.com/vincecao/castable-universal): a cast receiver for your
computer. Phones with a DLNA cast button find it on the Wi-Fi and play videos on it, in the player
you choose. This repository holds the logic, the network layer, the player registry and the
command-line setup; the apps and screens live in `castable-universal`.

## Quick start

**macOS or Linux**

```sh
./scripts/setup.sh
```

**Windows** (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
```

The script installs what's missing (Rust, Node 22, pnpm; a player if you want one), builds the
receiver, and opens the setup wizard. The wizard is keyboard only: ↑↓ to move, Enter to choose,
Esc to go back. It asks for:

1. the name phones will show (for example "Living room")
2. the player: installed players first, with full control (mpv, IINA, VLC) before start-only ones
   (QuickTime, ffplay, MPC-HC, PotPlayer, the browser)
3. the network port: automatic, 49152, or one you type

Then it starts the receiver with a live status screen. Open a cast app on a phone on the same Wi-Fi
(mconnect or TV Assist on iPhone, BubbleUPnP on Android, or any video app with a DLNA/"TV" button)
and pick the name you chose.

## Everyday use

```sh
pnpm castable            # start (or set up, the first time)
pnpm castable setup      # change name, player or port
pnpm castable start      # start with the live status screen: q stop · l open the log · r write a report
pnpm castable players    # which players this computer has
```

The receiver also runs on its own, with flags that override the settings file:

```sh
target/release/castable-receiver --name "My Mac" --player mpv --port 49152
target/release/castable-receiver --player 'open -a "QuickTime Player" {url}'   # any command
target/release/castable-receiver --players
```

Settings live in `config.toml` in Castable's folder: `~/.config/castable` on macOS and Linux,
`%APPDATA%\castable` on Windows.

Players show titles and subtitles in a font that covers Chinese, Japanese and Korean as well as
Latin: PingFang SC on macOS, Microsoft YaHei on Windows, Noto Sans CJK SC on Linux. To use another,
add `font = "Songti SC"` to `config.toml`; `font = ""` keeps the player's own. When the phone sends
no title, the player shows the video's file name, decoded (`后会友期`, not `%E5%90%8E…`).

## Players

Players are data, not code. Each is an entry in
[`crates/castable-players/players.toml`](crates/castable-players/players.toml): a program, its
arguments with placeholders (`{url}`, `{title}`, `{referer}`...), and how to control it:

| control | what works | players |
| --- | --- | --- |
| `mpv-ipc` | play, pause, seek, volume, real loaded / progress / end | mpv, IINA |
| `vlc-http` | the same, through VLC's web remote | VLC |
| `launch` | start and stop only | QuickTime, ffplay, MPC-HC, PotPlayer, browser |

**Add a player** for yourself in `players.toml` in Castable's folder, or for everyone with a pull
request to the built-in file. A player that speaks one of the protocols above needs no code:

```toml
[players.celluloid]
description = "Celluloid (Linux, built on mpv), full control"
program = ["celluloid"]
args = ["--mpv-input-ipc-server={ipc}", "--mpv-pause", "{url}"]
control = "mpv-ipc"
platforms = ["linux"]
```

A typo is reported with the player's name when the receiver starts. A new protocol is a variant of
`Control` in `src/spec.rs` and a module like `src/mpv.rs`.

## Logs

Every line is a message plus named fields:

```
INFO control action="Seek"
INFO player command command="load" gen=1 url=https://… title="…" mime="video/mp4" live=false
INFO transport from="TRANSITIONING" to="PLAYING" gen=1
```

| Level | What it shows |
|---|---|
| `info` (default) | casts, sender actions, player commands and events, state changes, subscriptions |
| `debug` | also every HTTP request (peer, status, time in µs), SSDP search and status poll |
| `trace` | also playback progress, once a second |

- `RUST_LOG=debug castable-receiver`, or `RUST_LOG=castable_net::http=debug` for one part.
- `--log json` prints one JSON object per line; the status screen reads exactly this.
- A level that is off costs one check and builds nothing.

### Errors

Every error has a code, the same whichever player or phone hit it. The first digit is its category:
1xx player, 2xx network, 4xx media, 6xx phone commands, 7xx settings, 8xx this computer.

```
WARN player event event="load_failed" code="E403" error="forbidden" category="media" detail="[ffmpeg] https: HTTP error 403 Forbidden"
```

The full list is in [docs/ERRORS.md](docs/ERRORS.md), generated from the code.

### Reporting a problem

Every run also writes a detailed log, including what the player printed, to `logs/receiver.log` in
Castable's folder (the previous run's is `receiver.1.log`). To share it:

```sh
pnpm castable report        # or: castable-receiver --report [FILE]
```

While `pnpm castable start` runs, press `r` to write a report and open it, or `l` to open the log.

This writes `castable-report-….md` with your OS, settings, players, the error codes seen and the
last log lines. URL queries, passwords, tokens and cookies are replaced with `…`. Check it, then
attach it to an [issue](https://github.com/vincecao/castable-core/issues/new).
`--log-file PATH` moves the log; `--no-log-file` turns it off.

## Test without a phone

`scripts/fake-sender.sh` sends what a phone's cast button sends (macOS, Linux, or WSL/Git Bash):

```sh
cd ~/Movies && python3 -m http.server 8000 &          # serve a video from this computer
IP=$(ipconfig getifaddr en0)                           # fish: set IP (ipconfig getifaddr en0)

scripts/fake-sender.sh cast "http://$IP:8000/YOUR-VIDEO.mp4"
scripts/fake-sender.sh play
scripts/fake-sender.sh status        # CurrentTransportState: PLAYING
scripts/fake-sender.sh seek 0:01:30
```

`RECEIVER=http://host:port` points it at a receiver elsewhere (default `http://localhost:49152`).

## Structure

The full design (layers, the session state machine, the cast flow, players, logging) is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), a living document kept current with the code.

```
crates/castable-core/     LOGIC   session state machine, DLNA/UPnP parsing. Pure Rust, no I/O.
crates/castable-net/      I/O     SSDP discovery, HTTP/SOAP control, GENA events; the Player seam
crates/castable-players/  PLAYERS players.toml registry, control protocols, castable-receiver
packages/protocol/        WIRE    @castable/protocol: TypeScript mirrors of the core's JSON types
packages/cli/             CLI     @castable/cli: the setup wizard and live status (Ink)
fixtures/                         JSON written by the Rust tests, type-checked by TypeScript
models/cast-session/              TLA+ model of the cast session
models/system/                    TLA+ model of the whole system end to end (statechart charts in its README)
scripts/                          setup.sh, setup.ps1, fake-sender.sh
```

Design rules:

- **Discriminated unions everywhere.** State is one enum whose variants carry only the data valid
  in them (`Phase`, `Delivery`, `Action`, `PlayerCommand`, `PlayerEvent`, `Control`; the wizard's
  `Step`), mirrored as tagged unions in TypeScript. Invalid states can't be written down.
- **Pure transitions.** The session, the wizard and the status screen are reducers: input in, next
  state out. I/O lives at the edges (`castable-net`, `castable-players`, `cli/src/system.ts`).
- **Derive, don't store.** The UPnP transport state, the available actions and the event payloads
  are computed from the one state value.
- **One source per fact.** Services, URLs and state variables are defined once (`service.rs`);
  players once (`players.toml`); OS paths once (the receiver, which the CLI asks).
- **Model first.** The cast session is checked in TLA+, and the same rules are checked against the
  Rust code by an exhaustive test (`tests/session_model.rs`).

## Develop

```sh
cargo test                                      # Rust: unit, model, end-to-end, fixtures
pnpm install && pnpm typecheck && pnpm test     # TypeScript: wire types, CLI reducers
UPDATE_FIXTURES=1 cargo test --test fixtures    # after changing a shared type
cargo fmt && cargo clippy --all-targets -- -D warnings
```

Shell notes: the commands above are for bash/zsh; in fish, `VAR=value cmd` works, and
`IP=$(...)` is `set IP (...)`. If `cargo` isn't found after installing Rust in fish, run
`fish_add_path ~/.cargo/bin` once.

## License

MIT. The protocol design follows [LeanCast](https://github.com/addozhang/leancast) (Apache-2.0);
no code is copied.
