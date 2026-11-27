// castable: set up and run the receiver from the terminal.
//
//   castable            set up if there are no settings yet, else start
//   castable setup      the setup wizard (keyboard only)
//   castable start      run the receiver with a live status screen
//   castable players    list the players this computer has
//   castable report     write a report to attach to an issue

import { execFileSync } from "node:child_process";
import { render } from "ink";
import App, { type Mode } from "./screens/App.tsx";
import * as sys from "./system.ts";

const USAGE = "castable [setup | start | players | report [FILE]]";

/** The built receiver, or a message and exit. */
function requireReceiver(): string {
  const bin = sys.findReceiver();
  if (bin) return bin;
  console.error("castable-receiver isn't built yet: run `castable setup` first.");
  process.exit(1);
}

/** One handler per command. */
const COMMANDS: Record<string, (args: string[]) => Promise<void> | void> = {
  players: () => {
    execFileSync(requireReceiver(), ["--players"], { stdio: "inherit" });
  },
  // The receiver gathers it (it knows this OS's paths); this says where it went.
  report: (args) => {
    const file = execFileSync(requireReceiver(), ["--report", ...args], { encoding: "utf8" }).trim();
    console.log(`Report written to ${file}`);
    console.log("Check it, then attach it to an issue: https://github.com/vincecao/castable-core/issues/new");
  },
  setup: () => screens("setup"),
  start: () => screens("start"),
  "": () => {
    const bin = sys.findReceiver();
    const configured = bin !== null && sys.readSettings(sys.settingsPath(bin)).name !== undefined;
    return screens(configured ? "start" : "setup");
  },
};

async function screens(mode: Mode) {
  if (!process.stdin.isTTY) {
    console.error("castable needs a terminal for its keyboard screens: run it directly, not piped.");
    process.exit(1);
  }
  let saved = false;
  // Ctrl+C is handled by the screens, so the receiver can say goodbye first.
  const app = render(<App mode={mode} onSaved={() => (saved = true)} />, { exitOnCtrlC: mode === "setup" });
  await app.waitUntilExit();
  if (saved) console.log(" Saved. Run `castable start` when you want phones to find this computer.");
}

const [command = "", ...args] = process.argv.slice(2);
const run = COMMANDS[command];
if (run) await run(args);
else {
  console.log(USAGE);
  process.exit(command === "help" || command === "--help" ? 0 : 1);
}
