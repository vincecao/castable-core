// The live status screen as data: folds castable-receiver's JSON log lines
// (`--log json`) into what to show. The log fields are the contract; they are
// written in crates/castable-net (actor.rs, http.rs) and the receiver binary.
// One handler per log message, in a table: a new message is one entry.

import type { TransportState } from "@castable/protocol";
import { truncate } from "lodash-es";

export interface Ready {
  name: string;
  url: string;
  player: string;
  control: string;
  logFile: string;
}

/** Who sent the cast: the phone's address and its app, from the request's User-Agent. */
export interface Sender {
  ip: string;
  app: string;
}

/** The cast on screen. */
export interface Source {
  title: string;
  url: string;
  host: string;
  /** "hls", "mp4", ... or "" when unclear. */
  format: string;
  live: boolean;
  from: Sender | null;
}

/** What the player reported about the media (castable-core `MediaInfo`). */
export interface Quality {
  /** "1080p · 1920×1080 · H.264 · 23.98 fps · 5.2 Mb/s | AAC · 48 kHz · stereo" */
  summary: string;
  /** Hardware decoder in use, or "". */
  decoder: string;
}

export interface Position {
  secs: number;
  of: number | null;
}

export interface Recent {
  /** Local time, "12:54:03". */
  at: string;
  text: string;
  tone: "info" | "warn";
}

export interface Status {
  ready: Ready | null;
  state: TransportState;
  now: Source | null;
  quality: Quality | null;
  position: Position | null;
  /** The sender of the next cast: "cast from" comes just before the load. */
  sender: Sender | null;
  /** The most recent happenings, newest last. */
  recent: Recent[];
  /** The last warning or error, until the next cast. */
  problem: string | null;
}

export const initialStatus: Status = {
  ready: null,
  state: "NO_MEDIA_PRESENT",
  now: null,
  quality: null,
  position: null,
  sender: null,
  recent: [],
  problem: null,
};

type Line = Record<string, unknown>;
const str = (v: unknown) => (typeof v === "string" ? v : v === undefined || v === null ? "" : String(v));
const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : null);

const MAX_RECENT = 8;

/** "ExampleCast/1.0 (Linux; Android 14)" → "ExampleCast 1.0": the first product in a User-Agent. */
export function appOf(userAgent: string): string {
  const first = userAgent.trim().split(/\s+/)[0] ?? "";
  return truncate(first.replace("/", " "), { length: 32 });
}

/** 724 → "12:04", 3723 → "1:02:03". */
export function clock(secs: number): string {
  const t = Math.max(0, Math.floor(secs));
  const [h, m, s] = [Math.floor(t / 3600), Math.floor((t % 3600) / 60), t % 60];
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** "E403 forbidden: <detail or summary>" for a log line that carries an error code. */
export function errorText(line: Line): string | null {
  const code = str(line.code);
  if (!/^E\d+$/.test(code)) return null;
  const about = str(line.detail) || str(line.summary) || str(line.message);
  return `${code} ${str(line.error)}${about ? `: ${about}` : ""}`;
}

const timeOf = (line: Line) => {
  const t = new Date(str(line.timestamp));
  return Number.isNaN(t.getTime()) ? "" : t.toLocaleTimeString("en-GB", { hour12: false });
};

type Handler = (status: Status, line: Line, say: Say) => Status;
type Say = (text: string, patch?: Partial<Status>, tone?: Recent["tone"]) => Status;

/** One handler per log message. */
const HANDLERS: Record<string, Handler> = {
  ready: (s, l) => ({
    ...s,
    ready: { name: str(l.name), url: str(l.url), player: str(l.player), control: str(l.control), logFile: str(l.log_file) },
  }),
  renamed: (s, l, say) => say(`renamed: “${str(l.name)}”`, s.ready ? { ready: { ...s.ready, name: str(l.name) } } : {}),
  "rename refused": (_s, l, say) => say(`can't rename: ${str(l.detail)}`, {}, "warn"),
  // From the status screen itself (a shortcut's result), not the receiver.
  note: (_s, l, say) =>
    l.level === "WARN" ? say(str(l.text), { problem: str(l.text) }, "warn") : say(str(l.text)),
  "cast from": (s, l) => ({ ...s, sender: { ip: str(l.peer), app: appOf(str(l.user_agent)) } }),
  transport: (_s, l, say) => say(`${str(l.from)} → ${str(l.to)}`, { state: str(l.to) as TransportState }),
  "player command": (s, l, say) => {
    if (l.command !== "load") {
      const to = num(l.to_s);
      return say(`${str(l.command)}${to !== null ? ` to ${clock(to)}` : ""}`);
    }
    const now: Source = {
      title: str(l.title) || str(l.url),
      url: str(l.url),
      host: str(l.host),
      format: str(l.format),
      live: l.live === true,
      from: s.sender,
    };
    return say(`cast: ${now.title}`, { now, quality: null, position: null, sender: null, problem: null });
  },
  "media info": (s, l, say) => {
    const quality = { summary: str(l.summary), decoder: str(l.decoder) };
    return quality.summary === s.quality?.summary ? { ...s, quality } : say(`quality: ${quality.summary}`, { quality });
  },
  "player event": (s, l, say) => {
    switch (l.event) {
      case "progress":
        return { ...s, position: { secs: num(l.position_s) ?? 0, of: num(l.duration_s) } };
      case "load_failed": {
        const text = errorText(l) ?? "failed";
        return say(`failed: ${text}`, { problem: text }, "warn");
      }
      case "closed":
        return say("player closed: Play opens it again");
      default:
        return s;
    }
  },
  "control refused": (_s, l, say) => say(`refused ${str(l.action)}: ${errorText(l) ?? str(l.code)}`, {}, "warn"),
};

/** One log line in; the next status out. Lines that aren't JSON or don't matter change nothing. */
export function fold(status: Status, raw: string): Status {
  let line: Line;
  try {
    line = JSON.parse(raw) as Line;
  } catch {
    return status;
  }
  const say: Say = (text, patch = {}, tone = "info") => ({
    ...status,
    ...patch,
    recent: [...status.recent, { at: timeOf(line), text, tone }].slice(-MAX_RECENT),
  });
  const handler = HANDLERS[str(line.message)];
  if (handler) return handler(status, line, say);
  if (line.level === "WARN" || line.level === "ERROR") {
    const text = errorText(line) ?? str(line.message);
    return say(text, { problem: text }, "warn");
  }
  return status;
}
