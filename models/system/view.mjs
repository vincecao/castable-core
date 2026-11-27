export const title = "Castable end to end: phone, core, screen and player";

const core = {
  idle: "Waiting",
  loading: "Loading",
  stopped: "Stopped",
  playing: "Playing",
  paused: "Paused",
  unloaded: "Stopped, not loaded",
};
const player = {
  none: "player empty",
  loading: "player loading",
  paused: "player still",
  playing: "player playing",
  gone: "player closed",
};

export const state = (v) => {
  const cast = v.gen === 0 ? "" : v.gen === 1 ? " (video 1)" : " (video 2)";
  const own = v.pGen === v.gen || v.pState === "none" ? player[v.pState] : `${player[v.pState]}, old video`;
  const inFlight = v.cmdQ.length || v.evQ.length || v.uiQ.length ? " · messages in flight" : "";
  return `Core: ${core[v.phase]}${cast} · ${v.phase === "idle" ? player[v.pState] : own}${inFlight}`;
};

export const group = (v) =>
  v.phase === "idle" ? "idle" : v.phase === "loading" ? "loading" : v.phase === "playing" ? "playing" : "stopped";
export const colors = { idle: "tan", loading: "yellow", playing: "teal", stopped: "blue" };

const commandWords = { load: "load", play: "play", pause: "pause", stop: "stop", seek: "seek" };

export const step = (action, from, to) => {
  switch (action) {
    case "PhoneCasts":
      return from.gen === 0 ? "the phone casts a video" : "the phone casts a new video";
    case "PhoneSends":
      if (to.phase === "playing" || (to.phase === "loading" && to.then === "play")) return "the phone presses play";
      if (to.phase === "paused" || (to.phase === "loading" && to.then === "hold")) return "the phone presses pause";
      return "the phone presses stop";
    case "CorePress":
      return from.uiQ[1][2] !== from.gen ? "your press reaches the core, after the cast changed" : "your press reaches the core";
    case "CoreEvent": {
      const e = from.evQ[1];
      const late = e.g !== from.gen ? " (old video, ignored)" : "";
      return { loaded: "the core hears: loaded", failed: "the core hears: load failed", ended: "the core hears: ended", closed: "the core hears: player closed" }[e.e] + late;
    }
    case "PlayerTakes":
      return `the player gets: ${commandWords[from.cmdQ[1].c]}`;
    case "LoadDone":
      return "the player finishes loading";
    case "LoadFails":
      return "the video fails to load";
    case "VideoEnds":
      return "the video ends";
    case "WindowClosed":
      return "you close the player window";
    case "ViewArrives":
      return "the screen updates";
    case "YouPressPlay":
      return "you press play on the screen";
    case "YouPressPause":
      return "you press pause on the screen";
    case "YouSeek":
      return "you seek on the screen";
    default:
      return action;
  }
};
