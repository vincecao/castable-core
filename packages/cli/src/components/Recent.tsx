import { Box, Text, type TextProps } from "ink";
import { memo } from "react";
import type { Recent as RecentLine } from "../status.ts";

/** How each tone looks. */
const TONES = {
  info: { dimColor: true },
  warn: { color: "yellow" },
} as const satisfies Record<RecentLine["tone"], TextProps>;

/** The latest happenings, timestamped, warnings in yellow. */
function Recent({ lines }: { lines: readonly RecentLine[] }) {
  if (lines.length === 0) return null;
  return (
    <Box flexDirection="column" marginTop={1}>
      {lines.map((r, i) => (
        <Box key={i} gap={1}>
          <Text dimColor>{r.at}</Text>
          <Text {...TONES[r.tone]} wrap="truncate-end">
            {r.text}
          </Text>
        </Box>
      ))}
    </Box>
  );
}

export default memo(Recent);
