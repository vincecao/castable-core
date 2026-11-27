import { Box, Text } from "ink";
import { memo } from "react";
import { ACCENT } from "../theme.ts";

/** A line being typed, with a cursor. */
function Field({ label, value }: { label?: string; value: string }) {
  return (
    <Box gap={1}>
      <Text color={ACCENT}>{label ?? "›"}</Text>
      <Text>{value}</Text>
      <Text color={ACCENT}>▌</Text>
    </Box>
  );
}

export default memo(Field);
