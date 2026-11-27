# castable-core

The core of [Castable](https://github.com/vincecao/castable-universal): the logic of a DLNA cast
receiver, with no UI. Phones with a cast button find it on the network, send it a video, and
control playback; this repository decides what happens. The apps and screens live in
`castable-universal`, which uses this repository as a submodule.

```
crates/castable-core/  LOGIC  session state machine, DLNA/UPnP parsing. Pure Rust, no I/O.
crates/castable-net/   I/O    SSDP discovery, HTTP/SOAP control, GENA events (tokio)
packages/protocol/     WIRE   @castable/protocol: TypeScript mirrors of the core's JSON types
fixtures/                     JSON written by the Rust tests, type-checked by TypeScript
models/cast-session/          TLA+ model of the cast session
```

## Design rules

- **Discriminated unions everywhere.** State is one enum whose variants carry only the data valid
  in them (`Phase`, `Delivery`, `Action`, `Command`, `PlayerCommand`, `PlayerEvent`), mirrored as
  tagged unions in `@castable/protocol`. Invalid states can't be written down.
- **Derive, don't store.** The UPnP transport state, the available actions and the event payloads
  are computed from the one state value.
- **One table per fact.** Services, their URLs, actions and state variables are defined once
  (`service.rs`); the device description, SCPDs, SSDP and routing are generated from it.
- **Model first.** The cast session is checked in TLA+, and the same four rules are checked against
  the Rust code by an exhaustive test (`tests/session_model.rs`).
- **No UI here.** The core emits a `ReceiverView` for screens to draw and `PlayerCommand`s for a
  player to obey; it takes `PlayerEvent`s and `SenderCommand`s back.

## Test it locally

### 1. One-time setup (macOS)

```sh
curl https://sh.rustup.rs -sSf | sh     # Rust
brew install node@22 mpv                 # Node 22; mpv is optional, for real playback
corepack enable                          # pnpm
```

### 2. Run the tests

```sh
cargo test                       # unit, model, end-to-end and fixture tests
pnpm install && pnpm typecheck   # the TypeScript wire types
```

### 3. Cast from a real phone

```sh
cargo run -p castable-net --example receiver -- --name "My Mac" --mpv --port 49152
```

- Allow incoming connections when macOS asks.
- Put the phone on the same Wi-Fi and open a DLNA sender: mconnect Player or TV Assist on iPhone,
  BubbleUPnP on Android, or any video app with a DLNA/"TV" cast button.
- Pick "My Mac" and play a video. mpv plays it; the terminal prints each step (`▶ load`, `▶ play`).
  Without `--mpv` the receiver only logs what it would play.
- If an app misbehaves, run with `RUST_LOG=debug` in front and keep the output (see Logs).

### 4. No phone? Fake one

`scripts/fake-sender.sh` sends exactly what a phone's cast button sends. In a second terminal:

```sh
cd ~/Movies && python3 -m http.server 8000 &          # serve a video from this Mac
IP=$(ipconfig getifaddr en0)

scripts/fake-sender.sh cast "http://$IP:8000/YOUR-VIDEO.mp4"
scripts/fake-sender.sh play
scripts/fake-sender.sh status        # CurrentTransportState: PLAYING
scripts/fake-sender.sh seek 0:01:30
scripts/fake-sender.sh pause
```

Set `RECEIVER=http://other-host:port` to talk to a receiver elsewhere.

### Logs

Every line is a message plus named fields, so it can be filtered or parsed:

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

- `RUST_LOG=debug` raises the level; `RUST_LOG=castable_net::http=debug` raises it for one part.
- `--log-json` prints one JSON object per line, e.g. `... --log-json | jq 'select(.message=="transport")'`.
- A level that is off costs one check and builds nothing; no line formats the media or a whole struct.

## Develop

```sh
UPDATE_FIXTURES=1 cargo test --test fixtures    # after changing a shared type
cargo fmt && cargo clippy --all-targets -- -D warnings
```

## License

MIT. The protocol design follows [LeanCast](https://github.com/addozhang/leancast) (Apache-2.0);
no code is copied.
