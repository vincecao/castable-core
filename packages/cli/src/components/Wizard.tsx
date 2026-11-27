// The setup wizard's screens: one per step, picked by the step's kind.
import { Box, Text } from "ink";
import { memo } from "react";
import { MOVE_HINT } from "../theme.ts";
import { PORTS, reviewOptions, type State } from "../wizard.ts";
import Field from "./Field.tsx";
import Frame from "./Frame.tsx";
import List from "./List.tsx";
import Problem from "./Problem.tsx";

const playerNote = (c: State["choices"][number]) =>
  `${c.name === "log" ? "no player" : !c.installed ? "not installed" : c.control === "full" ? "full control" : "starts only"} · ${c.description}`;

function Wizard({ state, settingsPath }: { state: State; settingsPath: string }) {
  const { step, draft, choices, error } = state;
  switch (step.kind) {
    case "name":
      return (
        <Frame title="What should phones call this computer?" step="1 of 4" hint="Type a name · Enter next · Ctrl+C quit">
          <Field value={step.value} />
          <Text dimColor>It shows in the cast list on phones, like a TV's name.</Text>
          <Problem text={error} />
        </Frame>
      );
    case "player":
      return (
        <Frame title="Which player should play casts?" step="2 of 4" hint={MOVE_HINT}>
          <List cursor={step.cursor} items={choices.map((c) => ({ label: c.name, note: playerNote(c), muted: !c.installed }))} />
          <Problem text={error} />
        </Frame>
      );
    case "port":
      return (
        <Frame title="Which network port?" step="3 of 4" hint={step.typing === null ? MOVE_HINT : "Type digits · Enter next · Esc back"}>
          <List cursor={step.cursor} items={PORTS.map((p) => ({ label: p.label }))} />
          {step.typing !== null && (
            <Box marginTop={1}>
              <Field label="Port:" value={step.typing} />
            </Box>
          )}
          <Problem text={error} />
        </Frame>
      );
    case "review":
      return (
        <Frame title="Ready to save" step="4 of 4" hint={MOVE_HINT}>
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
    default: {
      const unhandled: never = step;
      return unhandled;
    }
  }
}

export default memo(Wizard);
