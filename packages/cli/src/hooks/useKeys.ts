import { useInput } from "ink";
import type { Key } from "../wizard.ts";

/** Arrow keys, Enter, Esc, Backspace and typed characters, as reducer keys. */
export default function useKeys(onKey: (k: Key) => void, active = true) {
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
