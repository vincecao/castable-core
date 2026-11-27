// The CLI's screens (Ink). They render a State or a Status and turn keys
// into reducer input; nothing here decides what happens next.

import { Box, Text, useInput } from "ink";
import type { ReactNode } from "react";
import { PORTS, reviewOptions, type Key, type State } from "./wizard.ts";
import type { Status } from "./status.ts";

const ACCENT = "#5b66ff"; // the receiver's cobalt, lifted for terminals

/** Arrow keys, Enter, Esc, Backspace and typed characters, as reducer keys. */
export function useKeys(onKey: (k: Key) => void, active = true) {
  useInput(
    (input, key) => {
      if (key.upArrow) onKey({ type: "up" });
      else if (key.downArrow) onKey({ type: "down" });
      else if (key.return) onKey({ type: "enter" });
      else if (key.escape || key.leftArrow) onKey({ type: "back" });
      else if (key.backspace || key.delete) onKey({ type: "erase" });
      else if (input && !key.ctrl && !key.meta) onKey({ type: "char", char: input });
    },
    { isActive: active },
  );
}

export function Frame({ title, step, children, hint }: { title: string; step?: string; children: ReactNode; hint: string }) {
  return (
    <Box flexDirection="column" paddingX={1}>
      <Box gap={1}>
        <Text color={ACCENT} bold>
          ◉ Castable
        </Text>
        <Text dimColor>{step}</Text>
      </Box>
      <Box marginTop={1} marginBottom={1}>
        <Text bold>{title}</Text>
      </Box>
      {children}
      <Box marginTop={1}>
        <Text dimColor>{hint}</Text>
      </Box>
    </Box>
  );
}

function List({ items, cursor }: { items: { label: string; note?: string; muted?: boolean }[]; cursor: number }) {
  return (
    <Box flexDirection="column">
      {items.map((item, i) => (
        <Box key={item.label} gap={1}>
          <Text color={i === cursor ? ACCENT : "white"}>{i === cursor ? "›" : " "}</Text>
          <Text bold={i === cursor} dimColor={!!item.muted && i !== cursor}>
            {item.label}
          </Text>
          {item.note && <Text dimColor>{item.note}</Text>}
        </Box>
      ))}
    </Box>
  );
}

const MOVE = "↑↓ move · Enter choose · Esc back · Ctrl+C quit";

export function Wizard({ state, settingsPath }: { state: State; settingsPath: string }) {
  const { step, draft, choices, error } = state;
  const err = error && (
    <Box marginTop={1}>
      <Text color="yellow">{error}</Text>
    </Box>
  );
  switch (step.kind) {
    case "name":
      return (
        <Frame title="What should phones call this computer?" step="1 of 4" hint="Type a name · Enter next · Ctrl+C quit">
          <Box gap={1}>
            <Text color={ACCENT}>›</Text>
            <Text>{step.value}</Text>
            <Text color={ACCENT}>▌</Text>
          </Box>
          <Text dimColor>It shows in the cast list on phones, like a TV's name.</Text>
          {err}
        </Frame>
      );
    case "player":
      return (
        <Frame title="Which player should play casts?" step="2 of 4" hint={MOVE}>
          <List
            cursor={step.cursor}
            items={choices.map((c) => ({
              label: c.name,
              note: `${c.name === "log" ? "no player" : !c.installed ? "not installed" : c.control === "full" ? "full control" : "starts only"} · ${c.description}`,
              muted: !c.installed,
            }))}
          />
          {err}
        </Frame>
      );
    case "port":
      return (
        <Frame
          title="Which network port?"
          step="3 of 4"
          hint={step.typing === null ? MOVE : "Type digits · Enter next · Esc back"}
        >
          <List cursor={step.cursor} items={PORTS.map((p) => ({ label: p.label }))} />
          {step.typing !== null && (
            <Box gap={1} marginTop={1}>
              <Text>Port:</Text>
              <Text>{step.typing}</Text>
              <Text color={ACCENT}>▌</Text>
            </Box>
          )}
          {err}
        </Frame>
      );
    case "review":
      return (
        <Frame title="Ready to save" step="4 of 4" hint={MOVE}>
          <Box flexDirection="column" marginBottom={1}>
            <Text>
              Name <Text bold>{draft.name}</Text>
            </Text>
            <Text>
              Player <Text bold>{draft.player}</Text>
            </Text>
            <Text>
              Port <Text bold>{draft.port === 0 ? "automatic" : draft.port}</Text>
            </Text>
            <Text dimColor>Saved to {settingsPath}</Text>
          </Box>
          <List cursor={step.cursor} items={reviewOptions.map((label) => ({ label }))} />
        </Frame>
      );
    case "done":
      return null;
  }
}

const STATE_WORDS: Record<Status["state"], [string, string]> = {
  NO_MEDIA_PRESENT: ["Waiting for a cast", "gray"],
  TRANSITIONING: ["Loading", "yellow"],
  PLAYING: ["Playing", "green"],
  PAUSED_PLAYBACK: ["Paused", "cyan"],
  STOPPED: ["Stopped", "gray"],
};

export function Dashboard({ status }: { status: Status }) {
  const [word, color] = STATE_WORDS[status.state];
  if (!status.ready) {
    return (
      <Frame title="Starting the receiver…" hint="Ctrl+C or q quit">
        {status.problem && <Text color="yellow">{status.problem}</Text>}
      </Frame>
    );
  }
  return (
    <Frame title={`“${status.ready.name}” is ready`} hint="q stop and quit · phones on the same Wi-Fi can cast now">
      <Text dimColor>
        player {status.ready.player} ({status.ready.control}) · {status.ready.url}
      </Text>
      <Box marginTop={1} gap={1}>
        <Text color={color} bold>
          ● {word}
        </Text>
        {status.now && <Text>{status.now.title}</Text>}
      </Box>
      {status.problem && (
        <Box marginTop={1}>
          <Text color="yellow">{status.problem}</Text>
        </Box>
      )}
      {status.recent.length > 0 && (
        <Box flexDirection="column" marginTop={1}>
          {status.recent.map((r, i) => (
            <Text key={i} dimColor>
              {r}
            </Text>
          ))}
        </Box>
      )}
    </Frame>
  );
}
