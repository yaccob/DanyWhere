import type { ModelInfo } from "../../bindings";

/** DanyWhere: the line on the word list pages about the speech model in
 * effect there: the translation key and the model's display name. */
export interface WordListModelLine {
  key: string;
  model: string;
}

/** The speech model as the line needs it: its display name and whether it
 * takes the word lists as vocabulary. */
export type WordListModel = Pick<ModelInfo, "supports_vocabulary"> & {
  name: string;
};

/** Whether the model uses the word lists while it recognizes speech, or
 * leaves them to post-processing as known terms; no line without a model. */
export const wordListModelLine = (
  model: WordListModel | undefined,
): WordListModelLine | null => {
  if (!model) return null;
  return {
    key: model.supports_vocabulary
      ? "settings.advanced.wordLists.modelUses"
      : "settings.advanced.wordLists.modelIgnores",
    model: model.name,
  };
};
