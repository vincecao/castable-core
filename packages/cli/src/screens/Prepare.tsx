// Finds the receiver, building it first if this checkout has none.
import { Text, useApp, useInput } from "ink";
import { memo, useEffect, useState } from "react";
import Frame from "../components/Frame.tsx";
import Problem from "../components/Problem.tsx";
import * as sys from "../system.ts";

function Prepare({ onReady }: { onReady: (bin: string) => void }) {
  const [building, setBuilding] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [failed, setFailed] = useState<string | null>(null);
  const { exit } = useApp();
  useInput((input) => input === "q" && exit(), { isActive: failed !== null });
  useEffect(() => {
    const found = sys.findReceiver();
    if (found) return onReady(found);
    if (!sys.hasCargo()) {
      setFailed("Rust isn't installed, so the receiver can't be built. Run scripts/setup, then try again.");
      return;
    }
    setBuilding(true);
    void sys.buildReceiver((l) => setLines((prev) => [...prev, l].slice(-6))).then((ok) => {
      const bin = ok ? sys.findReceiver() : null;
      if (bin) onReady(bin);
      else setFailed("The build failed. The lines above say why.");
    });
  }, []);
  if (!building && !failed) return null;
  return (
    <Frame title={failed ? "Can't start yet" : "Building the receiver (first run only)…"} hint={failed ? "q quit" : "This takes a minute or two"}>
      {lines.map((l, i) => (
        <Text key={i} dimColor wrap="truncate">
          {l}
        </Text>
      ))}
      <Problem text={failed} />
    </Frame>
  );
}

export default memo(Prepare);
