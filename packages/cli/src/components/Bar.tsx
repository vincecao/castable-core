import { Text } from "ink";
import { clamp } from "lodash-es";
import { memo } from "react";
import { ACCENT } from "../theme.ts";

/** A progress bar for a 0..1 fraction. */
function Bar({ fraction, width = 30 }: { fraction: number; width?: number }) {
  const filled = Math.round(clamp(fraction, 0, 1) * width);
  return (
    <Text>
      <Text color={ACCENT}>{"█".repeat(filled)}</Text>
      <Text dimColor>{"░".repeat(width - filled)}</Text>
    </Text>
  );
}

export default memo(Bar);
