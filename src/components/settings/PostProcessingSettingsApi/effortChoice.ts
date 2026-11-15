// DanyWhere: what the effort choice next to the post-processing model shows
// (src-tauri/src/post_process_effort.rs knows which levels a model takes).
import type { EffortOptions } from "@/bindings";

/** Every level, from least to most effort, as the backend names them. */
export const EFFORT_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

/** The model's default: no effort goes with the request. */
export const MODEL_DEFAULT_EFFORT = "";

export interface EffortChoice {
  /** The model's default first, then the levels the model takes. */
  values: string[];
  /** The value the choice shows. */
  selected: string;
  /** The endpoint refused the selected level while the app runs. */
  refused: boolean;
  /** The model is known to take no effort at all. */
  noEffort: boolean;
  /** Nothing is known about the model; the levels are the common ones. */
  guessed: boolean;
}

export const effortChoice = (
  options: EffortOptions | null,
  stored: string | null | undefined,
): EffortChoice => {
  const known = (EFFORT_LEVELS as readonly string[]).includes(stored ?? "")
    ? (stored as string)
    : MODEL_DEFAULT_EFFORT;
  if (!options) {
    // Until the options arrive, keep showing what is stored.
    const values = [MODEL_DEFAULT_EFFORT];
    if (known) values.push(known);
    return {
      values,
      selected: known,
      refused: false,
      noEffort: false,
      guessed: false,
    };
  }
  const guessed = options.source === "common";
  // Nothing is known about the model, so a stored level may well be one it
  // takes: keep it on offer.
  const offered = EFFORT_LEVELS.filter(
    (level) => options.levels.includes(level) || (guessed && level === known),
  );
  const values = [MODEL_DEFAULT_EFFORT, ...offered];
  const selected = values.includes(known) ? known : MODEL_DEFAULT_EFFORT;
  return {
    values,
    selected,
    refused:
      selected !== MODEL_DEFAULT_EFFORT && options.rejected.includes(selected),
    noEffort: !guessed && options.levels.length === 0,
    guessed,
  };
};
