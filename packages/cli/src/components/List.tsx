import { Box, Text } from "ink";
import { memo } from "react";
import { ACCENT } from "../theme.ts";

export interface ListItem {
  label: string;
  note?: string;
  muted?: boolean;
}

/** A keyboard list: the cursor's row is marked and bold. */
function List({ items, cursor }: { items: readonly ListItem[]; cursor: number }) {
  return (
    <Box flexDirection="column">
      {items.map((item, i) => {
        const on = i === cursor;
        return (
          <Box key={item.label} gap={1}>
            <Text color={on ? ACCENT : "white"}>{on ? "›" : " "}</Text>
            <Text bold={on} dimColor={!!item.muted && !on}>
              {item.label}
            </Text>
            {item.note && <Text dimColor>{item.note}</Text>}
          </Box>
        );
      })}
    </Box>
  );
}

export default memo(List);
