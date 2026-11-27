export const title = "Receiver cast session";
const status = { NoMedia: "Waiting", Transitioning: "Loading", Playing: "Playing", Paused: "Paused", Stopped: "Stopped" };
export const state = (v) => {
  if (v.uri === 0) return "Waiting for a cast";
  let screen;
  if (v.shown === 0) screen = "screen blank";
  else if (v.shown !== v.uri) screen = v.playing ? "old video still playing" : "old video on screen";
  else screen = v.playing ? "video playing" : "video ready";
  const pending = v.asked && !v.playing ? ", Play waiting" : "";
  return `${status[v.state]} · ${screen}${pending}`;
};
export const group = (v) => (v.uri === 0 ? "idle" : v.state === "Transitioning" ? "loading" : v.playing ? "playing" : "stopped");
export const colors = { idle: "tan", loading: "yellow", playing: "teal", stopped: "blue" };
export const step = (action, from, to) => {
  switch (action) {
    case "SenderCasts": return from.uri === 0 ? "phone casts a video" : "phone casts a new video";
    case "SenderPlays": return "you press Play";
    case "SenderPauses": return "you press Pause";
    case "SenderStops": return "you press Stop";
    case "LoadDone": return to.shown !== to.uri ? "the old video finishes loading" : "the video finishes loading";
    case "LoadFails": return "the video fails to load";
    case "VideoEnds": return "the video ends";
    default: return action;
  }
};
