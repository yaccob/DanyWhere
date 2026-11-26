// DanyWhere: the versions of a History entry (`history_versions.rs`). An
// entry is a stack of texts, bottom to top: the transcription, and every
// post-processing (✨) and saved edit (✎) put on top of the version it was
// made from (user, 2026-11-19, H1). The top is the entry's result. All
// versions are alike (H3): any can be moved anywhere and deleted, the
// transcription too, as long as one is left, and nothing asks first (H4).
// These helpers decide what History shows and offers for a stack; the
// backend changes it.
import type {
  HistoryEntry,
  HistoryVersion,
  SaveVersionMode,
  VersionStep,
} from "@/bindings";
import { matchRanges, type TextMatcher } from "./historySearch";

type Stack = Pick<HistoryEntry, "versions">;

/** The index of the top version, the entry's result. */
export const topIndex = (entry: Stack): number =>
  Math.max(0, entry.versions.length - 1);

/** The text of version `index`; empty for a version the stack doesn't
 * have. */
export const versionText = (entry: Stack, index: number): string =>
  entry.versions[index]?.text ?? "";

/** The entry's result, the top version's text: what History copies and
 * pastes again. */
export const entryText = (entry: Stack): string =>
  versionText(entry, topIndex(entry));

/** Whether the entry has a text to show, copy and edit: not while its
 * transcription is still to come or failed. */
export const hasText = (entry: Stack): boolean =>
  entryText(entry).trim().length > 0;

/** The versions the search's text matches, bottom to top (the track marks
 * them). None without a text to search or with a pattern JavaScript can't
 * read (`matcher` null); the backend finds the entry all the same. */
export const matchingVersions = (
  entry: Stack,
  matcher: TextMatcher | null,
): number[] => {
  if (!matcher) return [];
  const found: number[] = [];
  entry.versions.forEach((version, index) => {
    if (matchRanges(version.text, matcher).length > 0) found.push(index);
  });
  return found;
};

/** Whether the stack has version `index`. */
const hasVersion = (entry: Stack, index: number | null): index is number =>
  index !== null &&
  Number.isInteger(index) &&
  index >= 0 &&
  index < entry.versions.length;

/** The version an entry shows: the one the user `chosen` on the track, else
 * the top (a choice the stack no longer has counts as none). During a search
 * it shows that version if it matches, else the topmost version that does;
 * with no match at all it stays. Once the user chose a version during this
 * search (`pinned`), it shows their choice whatever matches. */
export const shownVersion = (
  entry: Stack,
  matcher: TextMatcher | null,
  chosen: number | null,
  pinned = false,
): number => {
  const choice = hasVersion(entry, chosen) ? chosen : topIndex(entry);
  if (pinned || !matcher) return choice;
  const matching = matchingVersions(entry, matcher);
  if (matching.length === 0 || matching.includes(choice)) return choice;
  return matching[matching.length - 1];
};

/** How saving the user's edit of version `base` can go: only the top
 * changes in place, whatever made it; an edit of any version can be added
 * on top as a new version. */
export const saveModes = (entry: Stack, base: number): SaveVersionMode[] =>
  hasVersion(entry, base) && base === topIndex(entry)
    ? ["replace", "new"]
    : ["new"];

/** The save History offers first (highlighted, on ⌘↩): changing the top
 * when its last step was the user's, so small fixes don't grow the stack;
 * else a new version, which keeps the AI's or the speech model's text. */
export const defaultSaveMode = (
  entry: Stack,
  base: number,
): SaveVersionMode => {
  if (!saveModes(entry, base).includes("replace")) return "new";
  const steps = entry.versions[base].steps;
  return steps[steps.length - 1]?.by === "user" ? "replace" : "new";
};

/** What the current version's menu offers: to the top and up (below the
 * top), down (above the bottom), delete; nothing for an entry's only
 * version. */
export type VersionAction = "to_top" | "up" | "down" | "delete";

export const versionActions = (
  entry: Stack,
  index: number,
): VersionAction[] => {
  if (!hasVersion(entry, index) || entry.versions.length < 2) return [];
  const actions: VersionAction[] = [];
  if (index < topIndex(entry)) actions.push("to_top", "up");
  if (index > 0) actions.push("down");
  actions.push("delete");
  return actions;
};

/** Where version `index` moves to with `action`. */
export const moveTarget = (
  entry: Stack,
  index: number,
  action: Exclude<VersionAction, "delete">,
): number => {
  const top = topIndex(entry);
  if (action === "to_top") return top;
  if (action === "up") return Math.min(index + 1, top);
  return Math.max(index - 1, 0);
};

/** The place version `index` has after the one at `from` moved to `to`. */
export const indexAfterMove = (
  index: number,
  from: number,
  to: number,
): number => {
  if (index === from) return to;
  if (from < index && index <= to) return index - 1;
  if (to <= index && index < from) return index + 1;
  return index;
};

/** The place version `index` has after the one at `deleted` went; null
 * for that one itself. */
export const indexAfterDelete = (
  index: number,
  deleted: number,
): number | null => {
  if (index === deleted) return null;
  return index > deleted ? index - 1 : index;
};

/** A chip's box on screen, in client pixels. */
export type ChipBox = {
  left: number;
  right: number;
  top: number;
  bottom: number;
};

/** The gap a chip dropped at (`x`, `y`) goes into, 0 (before the first
 * chip) to `boxes.length` (after the last): before the chip nearest to the
 * point, or after it when the point is past its middle in the reading
 * direction (`rtl`: right to left). The nearest row of chips counts first,
 * then the nearest chip in it. */
export const dropSlot = (
  boxes: ChipBox[],
  x: number,
  y: number,
  rtl = false,
): number => {
  if (boxes.length === 0) return 0;
  const away = (low: number, high: number, at: number) =>
    at < low ? low - at : at > high ? at - high : 0;
  let nearest = 0;
  let best = [Infinity, Infinity];
  boxes.forEach((box, index) => {
    const distance = [
      away(box.top, box.bottom, y),
      away(box.left, box.right, x),
    ];
    if (
      distance[0] < best[0] ||
      (distance[0] === best[0] && distance[1] < best[1])
    ) {
      nearest = index;
      best = distance;
    }
  });
  const box = boxes[nearest];
  const middle = (box.left + box.right) / 2;
  const after = rtl ? x < middle : x > middle;
  return after ? nearest + 1 : nearest;
};

/** The place a version dragged from `from` gets when dropped into gap
 * `slot` (see `dropSlot`). */
export const dropIndex = (from: number, slot: number): number =>
  slot > from ? slot - 1 : slot;

/** How far a pressed chip moves before it is dragged, in pixels. */
export const DRAG_THRESHOLD = 4;

/** A pressed chip: where it came from, and the gap it goes into once it
 * moved far enough to be dragged (`active`). */
export type ChipDrag = {
  from: number;
  pointerId: number;
  startX: number;
  startY: number;
  active: boolean;
  slot: number;
};

export const pressChip = (
  from: number,
  pointerId: number,
  x: number,
  y: number,
): ChipDrag => ({
  from,
  pointerId,
  startX: x,
  startY: y,
  active: false,
  slot: from,
});

/** The drag once the pointer is at (`x`, `y`); the same object while
 * nothing changes. `slotAt` finds the gap under the point (`dropSlot`). */
export const moveChip = (
  drag: ChipDrag,
  x: number,
  y: number,
  slotAt: (x: number, y: number) => number,
): ChipDrag => {
  const distance = Math.hypot(x - drag.startX, y - drag.startY);
  if (!drag.active && distance < DRAG_THRESHOLD) return drag;
  const slot = slotAt(x, y);
  return drag.active && slot === drag.slot
    ? drag
    : { ...drag, active: true, slot };
};

/** What the release does: the move (none when the chip didn't move far
 * enough or went back to its place), and whether the click that follows
 * must not show the chip. */
export const releaseChip = (
  drag: ChipDrag | null,
): { move: { from: number; to: number } | null; swallowClick: boolean } => {
  if (!drag || !drag.active) return { move: null, swallowClick: false };
  const to = dropIndex(drag.from, drag.slot);
  return {
    move: to === drag.from ? null : { from: drag.from, to },
    swallowClick: true,
  };
};

/** Escape during a drag: the drag ends without a move, and a chip that
 * was being dragged isn't shown by the click its release makes. */
export const cancelChipDrag = (
  drag: ChipDrag | null,
): { swallowClick: boolean } => ({ swallowClick: drag?.active === true });

/** What the track shows for a version: the transcription, ✨ (post-processed),
 * ✎ (the user's), or ✨✎ when both made it. */
export type VersionMark = "transcription" | "ai" | "user" | "ai_user";

/** A step as the chip's tooltip names it: the key under
 * `settings.history.versions.step`, the prompt's name, the id of the speech
 * model that transcribed (when known), and its time in seconds, as
 * `formatDateTime` takes it. */
export type StepDescription = {
  key: "transcription" | "transcriptionModel" | "ai" | "aiPrompt" | "user";
  prompt: string | null;
  model: string | null;
  atSeconds: number | null;
};

const stepKey = (
  by: VersionStep["by"],
  prompt: string | null,
  model: string | null,
): StepDescription["key"] => {
  if (by === "ai") return prompt !== null ? "aiPrompt" : "ai";
  if (by === "transcription" && model !== null) return "transcriptionModel";
  return by;
};

export const describeStep = (step: VersionStep): StepDescription => {
  const prompt =
    step.by === "ai" && step.prompt_name?.trim() ? step.prompt_name : null;
  const model =
    step.by === "transcription" && step.model?.trim() ? step.model : null;
  return {
    key: stepKey(step.by, prompt, model),
    prompt,
    model,
    atSeconds: step.at === null ? null : Math.floor(step.at / 1000),
  };
};

/** Whether a step is a post-processing that ran with the instruction
 * spoken for the dictation: its line then says "… + spoken instruction". */
export const ranWithSpokenInstruction = (step: VersionStep): boolean =>
  step.by === "ai" && step.spoken_instruction === true;

/** The name a speech model shows under, from the app's model list; one
 * the list no longer has (deleted, or a custom model gone) by its id. */
export const modelName = <M extends { id: string; name: string }>(
  id: string,
  models: readonly M[],
  nameOf: (model: M) => string = (model) => model.name,
): string => {
  const model = models.find((candidate) => candidate.id === id);
  return model ? nameOf(model) : id;
};

/** When step `stepIndex` of version `index` was made, in seconds, for the
 * chip's tooltip. The backend gives the steps of an entry's own making the
 * entry's time; a transcription without one still gets it, wherever the
 * user moved it. Any other step without a time has none. */
export const stepSeconds = (
  entry: Pick<HistoryEntry, "versions" | "timestamp">,
  index: number,
  stepIndex: number,
): number | null => {
  const step = entry.versions[index]?.steps[stepIndex];
  if (!step) return null;
  const { atSeconds } = describeStep(step);
  if (atSeconds !== null) return atSeconds;
  return step.by === "transcription" ? entry.timestamp : null;
};

/** The lines of a chip's tooltip, a line repeated right after itself once:
 * two saves in place within a minute read the same. */
export const distinctLines = (lines: string[]): string[] =>
  lines.filter((line, index) => index === 0 || line !== lines[index - 1]);

/** ⌘Enter (Ctrl+Enter off macOS) saves an edit. */
export const isSaveShortcut = (event: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
}): boolean => event.key === "Enter" && (event.metaKey || event.ctrlKey);

export const versionMark = (version: HistoryVersion): VersionMark => {
  const ai = version.steps.some((step) => step.by === "ai");
  const user = version.steps.some((step) => step.by === "user");
  if (ai && user) return "ai_user";
  if (ai) return "ai";
  if (user) return "user";
  return "transcription";
};
