// Runs the receiver and shows its live status. Keys come from one table, so
// the hint line always matches what the keys do.
import { useApp, useInput, type Key } from "ink";
import { dirname } from "node:path";
import { memo, useEffect, useReducer, useRef, useState } from "react";
import Dashboard from "../components/Dashboard.tsx";
import { fold, initialStatus, type Status } from "../status.ts";
import * as sys from "../system.ts";

interface Context {
  bin: string;
  status: Status;
  stop: () => void;
  note: (text: string, warn?: boolean) => void;
  /** Starts editing the name (as the apps do when their name is clicked). */
  rename: () => void;
}

interface Shortcut {
  key: string;
  label: string;
  run: (ctx: Context) => void;
}

const SHORTCUTS: readonly Shortcut[] = [
  { key: "q", label: "stop and quit", run: ({ stop }) => stop() },
  { key: "n", label: "rename", run: ({ rename }) => rename() },
  {
    key: "l",
    label: "open the log",
    run: ({ status, note }) => {
      const log = status.ready?.logFile;
      if (!log) return note("no log file this run (--no-log-file?)", true);
      note(sys.openFile(log) ? `opened the log: ${log}` : `the log is at ${log}`);
    },
  },
  {
    key: "r",
    label: "write a report",
    run: ({ bin, status, note }) => {
      const log = status.ready?.logFile;
      try {
        const report = sys.writeReport(bin, log ? dirname(log) : process.cwd());
        note(sys.openFile(report) ? `report written and opened: ${report}` : `report written: ${report}`);
      } catch (e) {
        note(`couldn't write a report: ${e instanceof Error ? e.message : String(e)}`, true);
      }
    },
  },
];

/** Player keys, as in the apps (castable-core keys.rs): Ink's key names → the receiver's. */
function playerKey(input: string, key: Key): sys.PlayerKey | null {
  if (input === " " || input === "k") return "play-pause";
  if (key.leftArrow) return key.shift ? "step-back" : "seek-back";
  if (key.rightArrow) return key.shift ? "step-forward" : "seek-forward";
  if (input === "j") return "jump-back";
  if (input === "L") return "jump-forward"; // Shift+L (Ink gives "L"); plain l opens the log here
  if (key.upArrow) return "volume-up";
  if (key.downArrow) return "volume-down";
  if (input === "m") return "mute";
  if (input === "<" || input === ",") return "slower";
  if (input === ">" || input === ".") return "faster";
  if (key.escape || key.backspace) return "back";
  return null;
}

const RENAME_HINT = "type the new name · enter save · esc cancel";

const HINT = [
  "space play/pause · ←→ 5 s (shift 1 s) · ↑↓ volume · m mute · < > speed · esc back",
  SHORTCUTS.map((s) => `${s.key} ${s.label}`).join(" · "),
].join("\n");

function Run({ bin }: { bin: string }) {
  const [status, push] = useReducer(fold, initialStatus);
  const { exit } = useApp();
  const child = useRef<ReturnType<typeof sys.startReceiver> | null>(null);
  /** The name being typed, while renaming. */
  const [draft, setDraft] = useState<string | null>(null);
  useEffect(() => {
    const c = sys.startReceiver(bin, sys.settingsPath(bin), push);
    child.current = c;
    c.on("close", () => exit());
    return () => sys.stopReceiver(c);
  }, []);
  const ctx: Context = {
    bin,
    status,
    stop: () => child.current && sys.stopReceiver(child.current),
    note: (text, warn = false) => push(JSON.stringify({ level: warn ? "WARN" : "INFO", message: "note", text })),
    rename: () => setDraft(status.ready?.name ?? ""),
  };
  useInput((input, key) => {
    if (key.ctrl && input === "c") return ctx.stop();
    if (draft !== null) {
      if (key.escape) return setDraft(null);
      if (key.return) {
        if (child.current) sys.sendRename(child.current, draft);
        return setDraft(null);
      }
      if (key.backspace || key.delete) return setDraft(draft.slice(0, -1));
      if (key.ctrl || key.meta) return;
      // A chunk of input (fast typing, a paste) can mix text and backspaces: apply each in turn.
      setDraft([...input].reduce((d, c) => (c === "\x7f" || c === "\b" ? d.slice(0, -1) : c >= " " ? d + c : d), draft));
      return;
    }
    const player = playerKey(input, key);
    if (player && child.current) return sys.sendKey(child.current, player);
    SHORTCUTS.find((s) => s.key === input)?.run(ctx);
  });
  return <Dashboard status={status} hint={draft === null ? HINT : `Rename: ${draft}▌\n${RENAME_HINT}`} />;
}

export default memo(Run);
