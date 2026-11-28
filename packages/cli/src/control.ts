// How much of a cast a player can follow, as the receiver reports it (its
// `--players --json` and its `ready` line), and how the screens say it.

export type Control = "full" | "launch" | "none";

const TEXT: Record<Control, string> = { full: "full control", launch: "starts only", none: "log only" };

export const isControl = (c: string): c is Control => c in TEXT;

/** "full control", "starts only", "log only"; anything else as it came. */
export const controlText = (control: string): string => (isControl(control) ? TEXT[control] : control);
