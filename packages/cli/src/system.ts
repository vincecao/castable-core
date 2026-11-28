// Everything the CLI does to the machine: find or build castable-receiver,
// ask it for players and the settings path, read and write settings, run it.
// The receiver is the source of truth for anything OS-specific.

import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, delimiter } from "node:path";
import { hostname } from "node:os";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import type { PlayerInfo, Settings } from "./wizard.ts";

/** The castable-core checkout this CLI ships in. */
export const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const EXE = process.platform === "win32" ? ".exe" : "";

/** $CASTABLE_RECEIVER, then a built copy in this repo, then the PATH. */
export function findReceiver(): string | null {
  const candidates = [
    process.env.CASTABLE_RECEIVER,
    join(repoRoot, "target", "release", `castable-receiver${EXE}`),
    join(repoRoot, "target", "debug", `castable-receiver${EXE}`),
    ...(process.env.PATH ?? "").split(delimiter).map((d) => join(d, `castable-receiver${EXE}`)),
  ];
  return candidates.find((c): c is string => !!c && existsSync(c)) ?? null;
}

export function hasCargo(): boolean {
  try {
    execFileSync("cargo", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

/** Builds the receiver in release mode, streaming cargo's output lines. */
export function buildReceiver(onLine: (line: string) => void): Promise<boolean> {
  return new Promise((resolve) => {
    const cargo = spawn("cargo", ["build", "--release", "-p", "castable-players", "--bin", "castable-receiver"], {
      cwd: repoRoot,
      shell: process.platform === "win32",
    });
    for (const stream of [cargo.stdout, cargo.stderr]) {
      createInterface({ input: stream }).on("line", onLine);
    }
    cargo.on("error", () => resolve(false));
    cargo.on("close", (code) => resolve(code === 0));
  });
}

const ask = (bin: string, args: string[]) => execFileSync(bin, args, { encoding: "utf8" }).trim();

export const listPlayers = (bin: string): PlayerInfo[] => JSON.parse(ask(bin, ["--players", "--json"])) as PlayerInfo[];
export const settingsPath = (bin: string): string => ask(bin, ["--print-config-path"]);

/** Reads the keys the wizard writes; the receiver itself validates the file. */
export function readSettings(path: string): Partial<Settings> {
  if (!existsSync(path)) return {};
  const out: Partial<Settings> = {};
  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    const m = /^\s*(\w+)\s*=\s*(.+?)\s*$/.exec(line);
    if (!m) continue;
    const [, key, raw] = m as unknown as [string, string, string];
    const value = raw.startsWith('"') ? (JSON.parse(raw) as string) : raw;
    if (key === "name" || key === "player") out[key] = value;
    if (key === "port") out.port = Number(value);
    if (key === "log" && (value === "pretty" || value === "json")) out.log = value;
  }
  return out;
}

export function writeSettings(path: string, toml: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, toml);
}

export const defaultName = () => `Castable (${hostname().replace(/\.local$/, "")})`;

/**
 * Starts the receiver with its JSON log on stdout, one line per callback, and
 * its stdin open for player keys (`--control stdin`; see sendKey).
 */
export function startReceiver(bin: string, config: string, onLine: (line: string) => void): ChildProcess {
  // Progress (position, duration) is logged at trace; the status screen wants it.
  const env = { ...process.env, RUST_LOG: process.env.RUST_LOG ?? "info,castable_net::actor=trace" };
  const child = spawn(bin, ["--config", config, "--log", "json", "--control", "stdin"], {
    stdio: ["pipe", "pipe", "pipe"],
    env,
  });
  createInterface({ input: child.stdout! }).on("line", onLine);
  createInterface({ input: child.stderr! }).on("line", (l) =>
    onLine(JSON.stringify({ level: "ERROR", message: l })),
  );
  return child;
}

/**
 * The player keys the receiver understands (castable-core `keys.rs`): the same
 * table as the apps' screens, so a key does the same thing everywhere.
 */
export type PlayerKey =
  | "play-pause"
  | "seek-back"
  | "seek-forward"
  | "step-back"
  | "step-forward"
  | "jump-back"
  | "jump-forward"
  | "volume-up"
  | "volume-down"
  | "mute"
  | "slower"
  | "faster"
  | "back";

/** Presses a player key on the running receiver; it acts on what it shows now. */
export function sendKey(child: ChildProcess, key: PlayerKey): void {
  if (child.stdin?.writable) child.stdin.write(`${key}\n`);
}

/** Opens a file in its default app (a text editor or viewer). False if that can't start. */
export function openFile(path: string): boolean {
  const [cmd, args] =
    process.platform === "darwin"
      ? ["open", [path]]
      : process.platform === "win32"
        ? ["cmd", ["/c", "start", "", path]]
        : ["xdg-open", [path]];
  try {
    spawn(cmd, args as string[], { stdio: "ignore", detached: true }).on("error", () => {}).unref();
    return true;
  } catch {
    return false;
  }
}

/** Writes a report (the receiver gathers it; see `castable-receiver --report`) into `dir`; returns its path. */
export function writeReport(bin: string, dir: string): string {
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  return ask(bin, ["--report", join(dir, `castable-report-${stamp}.md`)]);
}

/** Stops the receiver so it can say goodbye on the network (SSDP byebye). */
export function stopReceiver(child: ChildProcess): void {
  if (process.platform === "win32") child.kill();
  else child.kill("SIGINT");
}
