// DanyWhere: corrections in History. An entry's transcription and its
// post-processed text can each be corrected; the originals stay, and the
// backend (`history_corrections.rs`) decides what an entry "is".
import type { HistoryEntry, ReprocessMode } from "@/bindings";

/** Which of an entry's two texts is meant. */
export type CorrectionPart = "transcription" | "post_processing";

type CorrectionFields = Pick<
  HistoryEntry,
  "transcription_text" | "post_processed_text"
> &
  Partial<
    Pick<HistoryEntry, "transcription_correction" | "post_processed_correction">
  >;

const nonBlank = (text: string | null | undefined): string | null =>
  text && text.trim().length > 0 ? text : null;

/** The part History shows: the post-processed text, unless the entry has
 * none or the original is asked for. */
export const shownPart = (
  entry: CorrectionFields,
  showOriginal: boolean,
): CorrectionPart =>
  nonBlank(entry.post_processed_text) !== null && !showOriginal
    ? "post_processing"
    : "transcription";

/** The user's correction of `part`, or null when there is none. */
export const correctionOf = (
  entry: CorrectionFields,
  part: CorrectionPart,
): string | null =>
  part === "transcription"
    ? nonBlank(entry.transcription_correction)
    : nonBlank(entry.post_processed_text) !== null
      ? nonBlank(entry.post_processed_correction)
      : null;

/** Whether the user corrected `part`. */
export const isCorrected = (
  entry: CorrectionFields,
  part: CorrectionPart,
): boolean => correctionOf(entry, part) !== null;

/** Whether running `mode` on the entry would replace a correction, so the
 * user is asked first. Post-processing again replaces only the correction of
 * the post-processed text; transcribing again replaces both. */
export const correctionsLostBy = (
  entry: CorrectionFields,
  mode: ReprocessMode,
): boolean =>
  mode === "post_process"
    ? isCorrected(entry, "post_processing")
    : isCorrected(entry, "transcription") ||
      isCorrected(entry, "post_processing");

/** ⌘Enter (Ctrl+Enter off macOS) saves a correction. */
export const isSaveShortcut = (event: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
}): boolean => event.key === "Enter" && (event.metaKey || event.ctrlKey);
