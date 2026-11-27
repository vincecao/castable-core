// The setup wizard: keys into the wizard's reducer; saves on done.
import { memo, useEffect, useReducer, useRef } from "react";
import Wizard from "../components/Wizard.tsx";
import useKeys from "../hooks/useKeys.ts";
import * as sys from "../system.ts";
import { initial, reduce, toToml, type State } from "../wizard.ts";

function Setup({ bin, onDone }: { bin: string; onDone: (start: boolean) => void }) {
  const path = useRef(sys.settingsPath(bin)).current;
  const [state, dispatch] = useReducer(reduce, undefined, (): State =>
    initial(sys.listPlayers(bin), sys.readSettings(path), sys.defaultName()),
  );
  useKeys(dispatch, state.step.kind !== "done");
  useEffect(() => {
    if (state.step.kind !== "done") return;
    sys.writeSettings(path, toToml(state.draft));
    onDone(state.step.start);
  }, [state.step]);
  return <Wizard state={state} settingsPath={path} />;
}

export default memo(Setup);
