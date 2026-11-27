// Runs the receiver and shows its live status. Keys come from one table, so
// the hint line always matches what the keys do.
import { useApp, useInput } from "ink";
import { dirname } from "node:path";
import { memo, useEffect, useReducer, useRef } from "react";
import Dashboard from "../components/Dashboard.tsx";
import { fold, initialStatus, type Status } from "../status.ts";
import * as sys from "../system.ts";

interface Context {
  bin: string;
  status: Status;
  stop: () => void;
  note: (text: string, warn?: boolean) => void;
}

interface Shortcut {
  key: string;
  label: string;
  run: (ctx: Context) => void;
}

const SHORTCUTS: readonly Shortcut[] = [
  { key: "q", label: "stop and quit", run: ({ stop }) => stop() },
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

const HINT = SHORTCUTS.map((s) => `${s.key} ${s.label}`).join(" · ");

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
  const ctx: Context = {
    bin,
    status,
    stop: () => child.current && sys.stopReceiver(child.current),
    note: (text, warn = false) => push(JSON.stringify({ level: warn ? "WARN" : "INFO", message: "note", text })),
  };
  useInput((input, key) => {
    if (key.ctrl && input === "c") return ctx.stop();
    SHORTCUTS.find((s) => s.key === input)?.run(ctx);
  });
  return <Dashboard status={status} hint={HINT} />;
}

export default memo(Run);
