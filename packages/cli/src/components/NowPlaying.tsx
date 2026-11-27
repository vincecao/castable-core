// What's playing: title, where it comes from, its quality and position.
import { Box, Text } from "ink";
import { memo } from "react";
import { clock, type Position, type Quality, type Source } from "../status.ts";
import { STATE_WORDS } from "../theme.ts";
import type { TransportState } from "@castable/protocol";
import Bar from "./Bar.tsx";
import Row from "./Row.tsx";

interface Props {
  state: TransportState;
  now: Source | null;
  quality: Quality | null;
  position: Position | null;
}

const sourceLine = (now: Source) =>
  [now.host, now.format.toUpperCase(), now.live ? "live" : "on demand"].filter(Boolean).join(" · ");

const fromLine = (from: NonNullable<Source["from"]>) => [from.app, from.ip].filter(Boolean).join(" · ");

function NowPlaying({ state, now, quality, position }: Props) {
  const { word, color } = STATE_WORDS[state];
  return (
    <Box flexDirection="column">
      <Box gap={1}>
        <Text color={color} bold>
          ● {word}
        </Text>
        {now && <Text bold>{now.title}</Text>}
      </Box>
      {now && (
        <Box flexDirection="column" marginTop={1}>
          <Row label="source">{sourceLine(now)}</Row>
          {now.from && <Row label="from">{fromLine(now.from)}</Row>}
          <Row label="quality">{quality?.summary || "waiting for the player…"}</Row>
          {quality?.decoder && <Row label="decoder">{`${quality.decoder} (hardware)`}</Row>}
          {position && (
            <Row label="position">
              {position.of && !now.live ? (
                <Box gap={1}>
                  <Bar fraction={position.secs / position.of} />
                  <Text>
                    {clock(position.secs)} / {clock(position.of)}
                  </Text>
                </Box>
              ) : (
                <Text>{clock(position.secs)}</Text>
              )}
            </Row>
          )}
        </Box>
      )}
    </Box>
  );
}

export default memo(NowPlaying);
