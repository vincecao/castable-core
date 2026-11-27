// Which screen: prepare the receiver, then set up or run.
import { useApp } from "ink";
import { memo, useEffect, useState } from "react";
import Prepare from "./Prepare.tsx";
import Run from "./Run.tsx";
import Setup from "./Setup.tsx";

export type Mode = "setup" | "start";
type Phase = Mode | "saved";

function App({ mode, onSaved }: { mode: Mode; onSaved: () => void }) {
  const [bin, setBin] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>(mode);
  const { exit } = useApp();
  useEffect(() => {
    if (phase !== "saved") return;
    onSaved();
    exit();
  }, [phase]);
  if (!bin) return <Prepare onReady={setBin} />;
  switch (phase) {
    case "setup":
      return <Setup bin={bin} onDone={(start) => setPhase(start ? "start" : "saved")} />;
    case "start":
      return <Run bin={bin} />;
    case "saved":
      return null;
  }
}

export default memo(App);
