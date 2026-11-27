import { test } from "node:test";
import assert from "node:assert/strict";
import { initial, reduce, toToml, playerChoices, type Key, type PlayerInfo, type State } from "../src/wizard.ts";

const players: PlayerInfo[] = [
  { name: "ffplay", description: "", control: "launch", availableHere: true, installed: true },
  { name: "iina", description: "", control: "full", availableHere: false, installed: false },
  { name: "mpv", description: "", control: "full", availableHere: true, installed: true },
  { name: "vlc", description: "", control: "full", availableHere: true, installed: false },
];

const press = (s: State, ...keys: (Key | string)[]) =>
  keys.reduce<State>(
    (st, k) =>
      reduce(
        st,
        typeof k === "string"
          ? k === "\n"
            ? { type: "enter" }
            : { type: "char", char: k }
          : k,
      ),
    s,
  );
const down: Key = { type: "down" };
const enter: Key = { type: "enter" };
const back: Key = { type: "back" };
const erase: Key = { type: "erase" };

test("installed, fully controllable players come first; other OSes are hidden", () => {
  assert.deepEqual(playerChoices(players).map((p) => p.name), ["mpv", "ffplay", "vlc", "log"]);
});

test("the happy path: keep the name, take mpv, automatic port, save and start", () => {
  const s = press(initial(players, {}, "Castable (mac)"), enter, enter, enter, enter);
  assert.deepEqual(s.step, { kind: "done", start: true });
  assert.deepEqual(s.draft, { name: "Castable (mac)", player: "mpv", port: 0, log: "pretty" });
});

test("typing a name, a custom port, and going back", () => {
  let s = initial(players, {}, "X");
  s = press(s, erase, "L", "i", "v", "i", "n", "g", " ", "r", "o", "o", "m", enter);
  assert.equal(s.draft.name, "Living room");
  s = press(s, down, enter); // ffplay
  assert.equal(s.draft.player, "ffplay");
  s = press(s, down, down, enter, "8", "0", enter); // type 80: refused
  assert.match(s.error ?? "", /1024 to 65535/);
  s = press(s, erase, erase, "4", "9", "1", "5", "3", enter);
  assert.equal(s.draft.port, 49153);
  s = press(s, back, back); // review -> port -> player
  assert.equal(s.step.kind, "player");
});

test("a player that isn't installed can't be picked", () => {
  const s = press(initial(players, {}, "X"), enter, down, down, enter); // vlc
  assert.equal(s.step.kind, "player");
  assert.match(s.error ?? "", /vlc isn't installed/);
});

test("an empty name is refused; existing settings are the defaults", () => {
  const s = press(initial(players, { name: "Den", player: "ffplay", port: 49152 }, "X"), erase, erase, erase, enter);
  assert.match(s.error ?? "", /Type a name/);
  const t = initial(players, { name: "Den", player: "ffplay", port: 49152 }, "X");
  const r = press(t, enter, enter, enter, down, enter);
  assert.deepEqual(r.step, { kind: "done", start: false });
  assert.deepEqual(r.draft, { name: "Den", player: "ffplay", port: 49152, log: "pretty" });
});

test("the settings file escapes quotes and backslashes", () => {
  const toml = toToml({ name: 'Lineng\'s "TV"', player: 'C:\\mpv {url}', port: 0, log: "json" });
  assert.match(toml, /name = "Lineng's \\"TV\\""/);
  assert.match(toml, /player = "C:\\\\mpv \{url\}"/);
});
