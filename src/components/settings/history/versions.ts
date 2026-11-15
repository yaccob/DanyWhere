// DanyWhere: the versions of a History entry (`history_versions.rs`). An
// entry is a stack: version 0 is the transcription, each version above was
// made from the one right below it (by post-processing or by the user), and
// the top is the entry's result. These helpers decide what History shows and
// offers for a stack; the backend changes it.
import type {
  HistoryEntry,
  HistoryVersion,
  ReprocessMode,
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

/** How saving the user's edit of version `base` can go: the transcription
 * is never replaced, so on it only as a new version; above it, changing
 * that version or keeping it and adding a new one. */
export const saveModes = (entry: Stack, base: number): SaveVersionMode[] =>
  base === 0 || !hasVersion(entry, base) ? ["new"] : ["replace", "new"];

/** The save History offers first (highlighted, on ⌘↩): changing the version
 * when its last step was the user's, so small fixes don't grow the stack;
 * else a new version, which keeps the AI's text (and the transcription). */
export const defaultSaveMode = (
  entry: Stack,
  base: number,
): SaveVersionMode => {
  if (!saveModes(entry, base).includes("replace")) return "new";
  const steps = entry.versions[base].steps;
  return steps[steps.length - 1]?.by === "user" ? "replace" : "new";
};

/** What ✨ can run on an entry. */
export type PostProcessAction = Extract<
  ReprocessMode,
  "post_process" | "post_process_replace" | "post_process_redo"
>;

/** The top version's steps; none when it's the transcription. */
const topSteps = (entry: Stack) =>
  topIndex(entry) > 0 ? (entry.versions[topIndex(entry)]?.steps ?? []) : [];

/** ✨'s actions, the click's first: post-processing the top further (always);
 * post-processing the top in place (not the transcription); and redoing the
 * post-processing from the version below (only when the AI made the top). */
export const postProcessActions = (entry: Stack): PostProcessAction[] => {
  const actions: PostProcessAction[] = ["post_process"];
  if (topIndex(entry) === 0) return actions;
  actions.push("post_process_replace");
  if (topSteps(entry)[0]?.by === "ai") actions.push("post_process_redo");
  return actions;
};

/** Redoing replaces the top: History asks first when that loses the user's
 * own steps in it. */
export const redoNeedsConfirm = (entry: Stack): boolean =>
  topSteps(entry).some((step) => step.by === "user");

/** How many versions go when the user edits version `base`: the ones above
 * it (History asks first when there are any). */
export const versionsLostByEditing = (entry: Stack, base: number): number =>
  Math.max(0, topIndex(entry) - base);

/** How many versions "Go Back to This Version" deletes for `keep`. */
export const versionsLostByGoingBack = (entry: Stack, keep: number): number =>
  Math.max(0, topIndex(entry) - keep);

/** Re-transcribing replaces the transcription and drops every version above
 * it; History asks first unless what comes back is what goes: no step of the
 * user's above the transcription, at most one version above it, and that one
 * comes back by post-processing (`mode` post-processes, or `transcribe` does
 * because the entry was post-processed). */
export const retranscribeNeedsConfirm = (
  entry: Pick<HistoryEntry, "versions" | "post_process_requested">,
  mode: ReprocessMode,
): boolean => {
  const above = entry.versions.slice(1);
  if (above.some((v) => v.steps.some((step) => step.by === "user"))) {
    return true;
  }
  if (above.length > 1) return true;
  return (
    above.length === 1 && mode === "transcribe" && !entry.post_process_requested
  );
};

/** What the track shows for a version: the transcription, ✨ (post-processed),
 * ✎ (the user's), or ✨✎ when both made it. */
export type VersionMark = "transcription" | "ai" | "user" | "ai_user";

/** A step as the chip's tooltip names it: the key under
 * `settings.history.versions.step`, the prompt's name, and its time in
 * seconds, as `formatDateTime` takes it. */
export type StepDescription = {
  key: "transcription" | "ai" | "aiPrompt" | "user";
  prompt: string | null;
  atSeconds: number | null;
};

export const describeStep = (step: VersionStep): StepDescription => {
  const prompt =
    step.by === "ai" && step.prompt_name?.trim() ? step.prompt_name : null;
  return {
    key: step.by === "ai" ? (prompt !== null ? "aiPrompt" : "ai") : step.by,
    prompt,
    atSeconds: step.at === null ? null : Math.floor(step.at / 1000),
  };
};

export const versionMark = (version: HistoryVersion): VersionMark => {
  const ai = version.steps.some((step) => step.by === "ai");
  const user = version.steps.some((step) => step.by === "user");
  if (ai && user) return "ai_user";
  if (ai) return "ai";
  if (user) return "user";
  return "transcription";
};
