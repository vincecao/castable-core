// castable: set up and run the receiver from the terminal.
//
//   castable            set up if there are no settings yet, else start
//   castable setup      the setup wizard (keyboard only)
//   castable start      run the receiver with a live status screen
//   castable players    list the players this computer has

import { render, Text, useApp, useInput } from "ink";
import { useEffect, useReducer, useRef, useState } from "react";
import { initial, reduce, toToml, type State } from "./wizard.ts";
import { fold, initialStatus } from "./status.ts";
import { Dashboard, Frame, Wizard, useKeys } from "./ui.tsx";
import * as sys from "./system.ts";

type Mode = "setup" | "start";

/** Finds the receiver, building it first if this checkout has none. */
function Prepare({ onReady }: { onReady: (bin: string) => void }) {
  const [building, setBuilding] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [failed, setFailed] = useState<string | null>(null);
  const { exit } = useApp();
  useInput((input) => input === "q" && exit(), { isActive: failed !== null });
  useEffect(() => {
    const found = sys.findReceiver();
    if (found) return onReady(found);
    if (!sys.hasCargo()) {
      setFailed("Rust isn't installed, so the receiver can't be built. Run scripts/setup, then try again.");
      return;
    }
    setBuilding(true);
    void sys.buildReceiver((l) => setLines((prev) => [...prev, l].slice(-6))).then((ok) => {
      const bin = ok ? sys.findReceiver() : null;
      if (bin) onReady(bin);
      else setFailed("The build failed. The lines above say why.");
    });
  }, []);
  if (!building && !failed) return null;
  return (
    <Frame title={failed ? "Can't start yet" : "Building the receiver (first run only)…"} hint={failed ? "q quit" : "This takes a minute or two"}>
      {lines.map((l, i) => (
        <Text key={i} dimColor wrap="truncate">
          {l}
        </Text>
      ))}
      {failed && <Text color="yellow">{failed}</Text>}
    </Frame>
  );
}

function Setup({ bin, onDone }: { bin: string; onDone: (start: boolean) => void }) {
  const path = useRef(sys.settingsPath(bin)).current;
  const [state, dispatch] = useReducer(reduce, undefined, (): State =>
    initial(sys.listPlayers(bin), sys.readSettings(path), sys.defaultName()),
  );
  useKeys(dispatch, state.step.kind !== "done");
  useEffect(() => {
    if (state.step.kind !== "done") return;
    sys.writeSettings(path, toToml(state.draft));
    onDone(state.step.start);
  }, [state.step]);
  return <Wizard state={state} settingsPath={path} />;
}

function Run({ bin }: { bin: string }) {
  const [status, push] = useReducer(fold, initialStatus);
  const { exit } = useApp();
  const child = useRef<ReturnType<typeof sys.startReceiver> | null>(null);
  useEffect(() => {
    const c = sys.startReceiver(bin, sys.settingsPath(bin), push);
    child.current = c;
    c.on("close", () => exit());
    return () => sys.stopReceiver(c);
  }, []);
  useInput((input, key) => {
    if (input === "q" || (key.ctrl && input === "c")) child.current && sys.stopReceiver(child.current);
  });
  return <Dashboard status={status} />;
}

function App({ mode, onSaved }: { mode: Mode; onSaved: () => void }) {
  const [bin, setBin] = useState<string | null>(null);
  const [phase, setPhase] = useState<Mode | "saved">(mode);
  const { exit } = useApp();
  useEffect(() => {
    if (phase === "saved") {
      onSaved();
      exit();
    }
  }, [phase]);
  if (!bin) return <Prepare onReady={setBin} />;
  if (phase === "setup") return <Setup bin={bin} onDone={(start) => setPhase(start ? "start" : "saved")} />;
  if (phase === "saved") return null;
  return <Run bin={bin} />;
}

let saved = false;
const [command = ""] = process.argv.slice(2);
if (command === "players") {
  const bin = sys.findReceiver();
  if (!bin) {
    console.error("castable-receiver isn't built yet: run `castable setup` first.");
    process.exit(1);
  }
  const { execFileSync } = await import("node:child_process");
  execFileSync(bin, ["--players"], { stdio: "inherit" });
} else if (["", "setup", "start"].includes(command)) {
  if (!process.stdin.isTTY) {
    console.error("castable needs a terminal for its keyboard screens: run it directly, not piped.");
    process.exit(1);
  }
  const bin = sys.findReceiver();
  const configured = bin !== null && sys.readSettings(sys.settingsPath(bin)).name !== undefined;
  const mode: Mode = command === "setup" || (command === "" && !configured) ? "setup" : "start";
  // Ctrl+C is handled by the screens, so the receiver can say goodbye first.
  const app = render(<App mode={mode} onSaved={() => (saved = true)} />, { exitOnCtrlC: mode === "setup" });
  await app.waitUntilExit();
  if (saved) console.log(" Saved. Run `castable start` when you want phones to find this computer.");
} else {
  console.log("castable [setup | start | players]");
  process.exit(command === "help" || command === "--help" ? 0 : 1);
}
