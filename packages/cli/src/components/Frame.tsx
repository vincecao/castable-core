import { Box, Text } from "ink";
import { memo, type ReactNode } from "react";
import { ACCENT } from "../theme.ts";

interface Props {
  title: string;
  step?: string;
  hint: string;
  children?: ReactNode;
}

/** Every screen's frame: brand, title, body, key hint. */
function Frame({ title, step, hint, children }: Props) {
  return (
    <Box flexDirection="column" paddingX={1}>
      <Box gap={1}>
        <Text color={ACCENT} bold>
          ◉ Castable
        </Text>
        {step && <Text dimColor>{step}</Text>}
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

export default memo(Frame);
