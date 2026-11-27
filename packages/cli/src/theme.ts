// Colours and words shared by the screens.
import type { TransportState } from "@castable/protocol";

export const ACCENT = "#5b66ff"; // the receiver's cobalt, lifted for terminals

export const STATE_WORDS: Record<TransportState, { word: string; color: string }> = {
  NO_MEDIA_PRESENT: { word: "Waiting for a cast", color: "gray" },
  TRANSITIONING: { word: "Loading", color: "yellow" },
  PLAYING: { word: "Playing", color: "green" },
  PAUSED_PLAYBACK: { word: "Paused", color: "cyan" },
  STOPPED: { word: "Stopped", color: "gray" },
};

export const MOVE_HINT = "↑↓ move · Enter choose · Esc back · Ctrl+C quit";
