// The live status screen as data: folds castable-receiver's JSON log lines
// (`--log json`) into what to show. The log fields are the contract; they
// are written in crates/castable-net/src/actor.rs and the receiver binary.

import type { TransportState } from "@castable/protocol";

export interface Status {
  ready: { name: string; url: string; player: string; control: string } | null;
  state: TransportState;
  /** The cast on screen: its title (or URL) and whether it's live. */
  now: { title: string; url: string } | null;
  /** The most recent happenings, newest last, as short lines. */
  recent: string[];
  /** The last warning or error, until the next cast. */
  problem: string | null;
}

export const initialStatus: Status = { ready: null, state: "NO_MEDIA_PRESENT", now: null, recent: [], problem: null };

type Line = Record<string, unknown>;
const str = (v: unknown) => (typeof v === "string" ? v : v === undefined || v === null ? "" : String(v));

const MAX_RECENT = 8;

/** A readable title: the sender's, else the file name, else the URL. */
export function titleOf(title: string, url: string): string {
  if (title) return title;
  try {
    const file = decodeURIComponent(new URL(url).pathname.split("/").filter(Boolean).pop() ?? "");
    return file.replace(/\.[a-z0-9]{2,5}$/i, "") || url;
  } catch {
    return url;
  }
}

/** "E403 forbidden: <detail or summary>" for a log line that carries an error code. */
export function errorText(line: Line): string | null {
  const code = str(line.code);
  if (!/^E\d+$/.test(code)) return null;
  const about = str(line.detail) || str(line.summary) || str(line.message);
  return `${code} ${str(line.error)}${about ? `: ${about}` : ""}`;
}

/** One log line in; the next status out. Lines that aren't JSON or don't matter change nothing. */
export function fold(status: Status, raw: string): Status {
  let line: Line;
  try {
    line = JSON.parse(raw) as Line;
  } catch {
    return status;
  }
  const say = (text: string, patch: Partial<Status> = {}): Status => ({
    ...status,
    ...patch,
    recent: [...status.recent, text].slice(-MAX_RECENT),
  });
  const level = str(line.level);
  switch (str(line.message)) {
    case "ready":
      return {
        ...status,
        ready: { name: str(line.name), url: str(line.url), player: str(line.player), control: str(line.control) },
      };
    case "transport":
      return say(`${str(line.from)} → ${str(line.to)}`, { state: str(line.to) as TransportState });
    case "player command":
      if (line.command === "load") {
        const title = titleOf(str(line.title), str(line.url));
        return say(`cast: ${title}`, { now: { title, url: str(line.url) }, problem: null });
      }
      return say(`${str(line.command)}${line.to_s !== undefined ? ` to ${Math.round(Number(line.to_s))}s` : ""}`);
    case "control refused":
      return say(`refused ${str(line.action)} (${str(line.code)})`);
    case "player event":
      if (line.event === "load_failed") {
        const text = errorText(line) ?? "failed";
        return say(`failed: ${text}`, { problem: text });
      }
      if (line.event === "closed") return say("player closed: Play opens it again");
      return status;
    default:
      if (level === "WARN" || level === "ERROR") {
        const text = errorText(line) ?? str(line.message);
        return say(text, { problem: text });
      }
      return status;
  }
}
