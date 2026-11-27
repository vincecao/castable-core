# Cast session model

A TLA+ model of the receiver's cast session: a sender casts, plays, pauses and
stops; the player loads asynchronously and can fail; a new cast can replace the
old one at any time.

- `naive.cfg` (`Fixed = FALSE`) models a straightforward receiver. It breaks all
  four rules.
- `fixed.cfg` (`Fixed = TRUE`) models the design implemented in
  `crates/castable-core/src/session.rs`. It holds all four rules with no deadlock
  (25 states).

| Rule | Meaning |
| --- | --- |
| `ScreenMatchesState` | What senders see matches what is on screen |
| `NeverShowsOldCast` | A replaced cast never stays on screen |
| `PlayIsNeverLost` | If you pressed Play, the video plays once it has loaded |
| `NeverStuckLoading` | "Loading" is shown only while something is really loading |

The same rules are checked against the real Rust code in
`crates/castable-core/tests/session_model.rs`, which replays each bug path and
searches every input sequence up to 14 steps.

## Run

Plain TLC (as CI does):

```sh
java -cp tla2tools.jar tlc2.TLC -config fixed.cfg -workers 1 CastReceiver.tla
```

With the statechart skill, to draw the chart and get each bug path:

```sh
statechart CastReceiver.tla --config naive.cfg --view view.mjs --out out-naive
```

The skill needs TLC 1.7.4; newer releases change SANY's XML output.
