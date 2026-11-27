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

/** Starts the receiver with its JSON log on stdout, one line per callback. */
export function startReceiver(bin: string, config: string, onLine: (line: string) => void): ChildProcess {
  const child = spawn(bin, ["--config", config, "--log", "json"], { stdio: ["ignore", "pipe", "pipe"] });
  createInterface({ input: child.stdout! }).on("line", onLine);
  createInterface({ input: child.stderr! }).on("line", (l) =>
    onLine(JSON.stringify({ level: "ERROR", message: l })),
  );
  return child;
}

/** Stops the receiver so it can say goodbye on the network (SSDP byebye). */
export function stopReceiver(child: ChildProcess): void {
  if (process.platform === "win32") child.kill();
  else child.kill("SIGINT");
}
