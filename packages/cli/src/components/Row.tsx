import { Box, Text } from "ink";
import { memo, type ReactNode } from "react";

/** A labelled line: a fixed-width dim label, then the value. */
function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Box gap={1}>
      <Box width={9} flexShrink={0}>
        <Text dimColor>{label}</Text>
      </Box>
      <Box flexGrow={1}>{typeof children === "string" ? <Text wrap="truncate-end">{children}</Text> : children}</Box>
    </Box>
  );
}

export default memo(Row);
