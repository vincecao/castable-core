// The setup wizard as data: a discriminated union of steps and one pure
// reducer. No I/O and no rendering here, so every key path is unit-tested.

export interface PlayerInfo {
  name: string;
  description: string;
  control: "full" | "launch";
  availableHere: boolean;
  installed: boolean;
}

export interface Settings {
  name: string;
  /** A player name, or "log" for none. */
  player: string;
  /** 0 picks a free port. */
  port: number;
  log: "pretty" | "json";
}

/** The ports offered: automatic, the common UPnP port, or typed. */
export const PORTS = [
  { label: "Automatic (a free port each time)", port: 0 },
  { label: "49152 (stays the same; easier for firewalls)", port: 49152 },
  { label: "Type a port", port: -1 },
] as const;

export type Step =
  | { kind: "name"; value: string }
  | { kind: "player"; cursor: number }
  | { kind: "port"; cursor: number; typing: string | null }
  | { kind: "review"; cursor: number }
  | { kind: "done"; start: boolean };

export interface State {
  step: Step;
  draft: Settings;
  /** Choices for the player step: installed players first, then "log". */
  choices: PlayerInfo[];
  error: string | null;
}

export type Key =
  | { type: "up" }
  | { type: "down" }
  | { type: "enter" }
  | { type: "back" }
  | { type: "char"; char: string }
  | { type: "erase" };

export const LOG_ONLY: PlayerInfo = {
  name: "log",
  description: "no player: only log what would play (for testing)",
  control: "launch",
  availableHere: true,
  installed: true,
};

/** Installed players first (full control before launch-only), then log-only. */
export function playerChoices(players: PlayerInfo[]): PlayerInfo[] {
  const rank = (p: PlayerInfo) => (p.installed ? 0 : 2) + (p.control === "full" ? 0 : 1);
  const usable = players.filter((p) => p.availableHere).sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  return [...usable, LOG_ONLY];
}

export function initial(players: PlayerInfo[], current: Partial<Settings>, defaultName: string): State {
  const choices = playerChoices(players);
  const draft: Settings = {
    name: current.name ?? defaultName,
    player: current.player ?? choices[0]?.name ?? "log",
    port: current.port ?? 0,
    log: current.log ?? "pretty",
  };
  return { step: { kind: "name", value: draft.name }, draft, choices, error: null };
}

const REVIEW = ["Save and start", "Save only", "Change something"] as const;
export const reviewOptions = REVIEW;

const wrap = (i: number, n: number) => (i + n) % n;

export function reduce(state: State, key: Key): State {
  const { step, draft, choices } = state;
  const go = (next: Step, patch: Partial<Settings> = {}): State => ({
    ...state,
    step: next,
    draft: { ...draft, ...patch },
    error: null,
  });
  const fail = (error: string): State => ({ ...state, error });

  switch (step.kind) {
    case "name":
      switch (key.type) {
        case "char":
          return go({ kind: "name", value: step.value + key.char });
        case "erase":
          return go({ kind: "name", value: step.value.slice(0, -1) });
        case "enter": {
          const name = step.value.trim();
          if (!name) return fail("Type a name phones will show, such as Living room.");
          const cursor = Math.max(0, choices.findIndex((c) => c.name === draft.player));
          return go({ kind: "player", cursor }, { name });
        }
        default:
          return state;
      }

    case "player":
      switch (key.type) {
        case "up":
          return go({ kind: "player", cursor: wrap(step.cursor - 1, choices.length) });
        case "down":
          return go({ kind: "player", cursor: wrap(step.cursor + 1, choices.length) });
        case "back":
          return go({ kind: "name", value: draft.name });
        case "enter": {
          const choice = choices[step.cursor];
          if (!choice) return state;
          if (!choice.installed) return fail(`${choice.name} isn't installed. Install it, or pick another player.`);
          const cursor = Math.max(0, PORTS.findIndex((p) => p.port === draft.port));
          return go({ kind: "port", cursor, typing: null }, { player: choice.name });
        }
        default:
          return state;
      }

    case "port":
      if (step.typing !== null) {
        switch (key.type) {
          case "char":
            return /\d/.test(key.char) && step.typing.length < 5 ? go({ ...step, typing: step.typing + key.char }) : state;
          case "erase":
            return go({ ...step, typing: step.typing.slice(0, -1) });
          case "back":
            return go({ ...step, typing: null });
          case "enter": {
            const port = Number(step.typing);
            if (!Number.isInteger(port) || port < 1024 || port > 65535) return fail("Pick a port from 1024 to 65535.");
            return go({ kind: "review", cursor: 0 }, { port });
          }
          default:
            return state;
        }
      }
      switch (key.type) {
        case "up":
          return go({ ...step, cursor: wrap(step.cursor - 1, PORTS.length) });
        case "down":
          return go({ ...step, cursor: wrap(step.cursor + 1, PORTS.length) });
        case "back":
          return go({ kind: "player", cursor: Math.max(0, choices.findIndex((c) => c.name === draft.player)) });
        case "enter": {
          const pick = PORTS[step.cursor];
          if (!pick) return state;
          return pick.port === -1 ? go({ ...step, typing: "" }) : go({ kind: "review", cursor: 0 }, { port: pick.port });
        }
        default:
          return state;
      }

    case "review":
      switch (key.type) {
        case "up":
          return go({ kind: "review", cursor: wrap(step.cursor - 1, REVIEW.length) });
        case "down":
          return go({ kind: "review", cursor: wrap(step.cursor + 1, REVIEW.length) });
        case "back":
          return go({ kind: "port", cursor: Math.max(0, PORTS.findIndex((p) => p.port === draft.port)), typing: null });
        case "enter":
          if (step.cursor === 2) return go({ kind: "name", value: draft.name });
          return go({ kind: "done", start: step.cursor === 0 });
        default:
          return state;
      }

    case "done":
      return state;
  }
}

/** The settings file, as castable-receiver reads it (crates/castable-players/src/settings.rs). */
export function toToml(s: Settings): string {
  const str = (v: string) => `"${v.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  return [
    "# Written by `castable setup`. Edit by hand or run it again.",
    `name = ${str(s.name)}`,
    `player = ${str(s.player)}`,
    `port = ${s.port}`,
    `log = ${str(s.log)}`,
    "",
  ].join("\n");
}
