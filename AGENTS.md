# Notes for coding agents

- Read `docs/ARCHITECTURE.md` before changing structure. It is the living design document.
- When a change alters what that file describes (a layer, a shared type, a session state or rule,
  the `Player` seam, the `players.toml` schema, config locations, CLI screens or log messages),
  update the matching section in the same commit.
- Keep the design rules in its §9: discriminated unions, pure transitions, derive don't store, one
  source per fact, model first.
- Before committing: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` and
  `pnpm typecheck && pnpm test`.
