import { Box, Text } from "ink";
import { memo } from "react";

/** A warning line, or nothing. */
function Problem({ text }: { text: string | null | undefined }) {
  if (!text) return null;
  return (
    <Box marginTop={1}>
      <Text color="yellow">{text}</Text>
    </Box>
  );
}

export default memo(Problem);
