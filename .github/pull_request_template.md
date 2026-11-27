## Summary

<!-- What changes and why. -->

## Testing

<!-- What you ran: cargo test, pnpm test, a real phone, … -->

## Checklist

- [ ] `docs/ARCHITECTURE.md` still describes the code (updated if this PR changes a layer, a shared type, a session state or rule, the `Player` seam, the `players.toml` schema, config locations or log messages)
- [ ] Shared types changed? Ran `UPDATE_FIXTURES=1 cargo test --test fixtures && pnpm typecheck`
- [ ] Session behaviour changed? Updated `models/cast-session/` and `tests/session_model.rs`
