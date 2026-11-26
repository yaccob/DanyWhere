import type {
  AppSettings,
  ModelInfo,
  PostProcessProvider,
} from "../../bindings";
import { APPLE_INTELLIGENCE_PROVIDER_ID } from "../../lib/providerLocation";
import { chosenProviderWithModel } from "./history/postProcessEntry";

/** What the word lists do with the speech model in effect: it takes them
 * as vocabulary; or post-processing (and the LLM check) gets them; or,
 * without a provider for that, they have no effect. */
export type WordListModelState = "vocabulary" | "postProcessing" | "noEffect";

/** DanyWhere: the line on the word list pages about the speech model in
 * effect there: the translation key, the model's display name and what
 * the page offers with it. */
export interface WordListModelLine {
  key: string;
  model: string;
  state: WordListModelState;
}

/** The speech model as the line needs it: its display name and whether it
 * takes the word lists as vocabulary. */
export type WordListModel = Pick<ModelInfo, "supports_vocabulary"> & {
  name: string;
};

const LINE_KEYS: Record<WordListModelState, string> = {
  vocabulary: "settings.advanced.wordLists.modelUses",
  postProcessing: "settings.advanced.wordLists.modelIgnores",
  noEffect: "settings.advanced.wordLists.modelIgnoresNoPostProcess",
};

/** Whether the model uses the word lists while it recognizes speech, or
 * leaves them to post-processing as known terms, which works only with a
 * provider that is ready (`llmReady`, see `wordListCheckSetup`); no line
 * without a model. */
export const wordListModelLine = (
  model: WordListModel | undefined,
  llmReady: boolean,
): WordListModelLine | null => {
  if (!model) return null;
  const state: WordListModelState = model.supports_vocabulary
    ? "vocabulary"
    : llmReady
      ? "postProcessing"
      : "noEffect";
  return { key: LINE_KEYS[state], model: model.name, state };
};

/** Whether the LLM check of the word lists (`word_list_llm_check`) can
 * ask an LLM in a scope, and which provider it asks. */
export type WordListCheckSetup =
  | { kind: "ready"; provider: PostProcessProvider }
  /** Apple Intelligence is chosen; it can't take the request. */
  | { kind: "unsupported"; provider: PostProcessProvider }
  /** No provider chosen, or none with a model. */
  | { kind: "missing" }
  /** Post-processing is set to "never": nothing goes to a provider. */
  | { kind: "never" };

type CheckSettings = Pick<
  AppSettings,
  | "post_process_provider_id"
  | "post_process_providers"
  | "post_process_models"
  | "post_process_mode"
>;

/** The backend's rule (`word_list_check::wanted`): post-processing has a
 * provider chosen with a model set, and it isn't Apple Intelligence.
 * Neither an instruction shortcut nor a prompt matters: the check is for
 * dictations that aren't post-processed. With post-processing set to
 * "never" nothing goes to a provider, the check neither (qa
 * 2026-11-26_1841, "Kleinere Folgen"); a mode not known doesn't stop it. */
export const wordListCheckSetup = (
  settings: Partial<CheckSettings> | null | undefined,
): WordListCheckSetup => {
  if (settings?.post_process_mode === "never") return { kind: "never" };
  const chosen = settings?.post_process_providers?.find(
    (p) => p.id === settings.post_process_provider_id,
  );
  if (chosen?.id === APPLE_INTELLIGENCE_PROVIDER_ID) {
    return { kind: "unsupported", provider: chosen };
  }
  const provider = chosenProviderWithModel(settings);
  return provider ? { kind: "ready", provider } : { kind: "missing" };
};
