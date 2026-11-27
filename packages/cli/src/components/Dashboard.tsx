// The live status screen: a Status, drawn.
import { Text } from "ink";
import { memo } from "react";
import type { Status } from "../status.ts";
import Frame from "./Frame.tsx";
import NowPlaying from "./NowPlaying.tsx";
import Problem from "./Problem.tsx";
import Recent from "./Recent.tsx";

function Dashboard({ status, hint }: { status: Status; hint: string }) {
  const { ready } = status;
  if (!ready) {
    return (
      <Frame title="Starting the receiver…" hint={hint}>
        <Problem text={status.problem} />
      </Frame>
    );
  }
  return (
    <Frame title={`“${ready.name}” is ready · phones on the same Wi-Fi can cast now`} hint={hint}>
      <Text dimColor>
        player {ready.player} ({ready.control}) · {ready.url}
      </Text>
      <Text> </Text>
      <NowPlaying state={status.state} now={status.now} quality={status.quality} position={status.position} />
      <Problem text={status.problem} />
      <Recent lines={status.recent} />
    </Frame>
  );
}

export default memo(Dashboard);
