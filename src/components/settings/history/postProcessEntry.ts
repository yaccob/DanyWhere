import type {
  AppSettings,
  PostProcessProvider,
  ReprocessMode,
} from "@/bindings";
import { hasActivePrompt } from "@/lib/promptLibrary";
import { postProcessingOn } from "@/lib/endKeys";

export type { ReprocessMode };

/** Whether post-processing can run with the current settings. */
export type PostProcessAvailability = "ready" | "off" | "not_set_up";

type ProviderSettings = Pick<
  AppSettings,
  "post_process_provider_id" | "post_process_providers" | "post_process_models"
>;

type PostProcessSettings = ProviderSettings &
  Pick<
    AppSettings,
    "post_process_mode" | "post_process_prompts" | "post_process_prompt_order"
  >;

/** DanyWhere: the chosen provider, when a model is set for it (the
 * backend's `active_post_process_provider` with a model); else `null`. */
export const chosenProviderWithModel = (
  settings: Partial<ProviderSettings> | null | undefined,
): PostProcessProvider | null => {
  const providerId = settings?.post_process_provider_id;
  const provider = settings?.post_process_providers?.find(
    (p) => p.id === providerId,
  );
  const model = providerId ? settings?.post_process_models?.[providerId] : "";
  return provider && model?.trim() ? provider : null;
};

/**
 * Mirrors what the backend checks before it calls the provider: a mode
 * other than "never" (DanyWhere, qa 2026-11-26_1810, "Schnittstelle Phase
 * 2"; the wand needs no instruction shortcut), and a provider with a model
 * and (DanyWhere, the prompt library) at least one active prompt.
 */
export const postProcessAvailability = (
  settings: Partial<PostProcessSettings> | null | undefined,
): PostProcessAvailability => {
  if (!settings || !postProcessingOn(settings)) return "off";
  const prompt = hasActivePrompt(
    settings.post_process_prompts,
    settings.post_process_prompt_order,
  );
  return chosenProviderWithModel(settings) && prompt ? "ready" : "not_set_up";
};

/** The tooltip of "Re-transcribe": what a click does. Handy post-processes
 * the new transcript of an entry that was post-processed before
 * (`post_process_requested`, `ReprocessMode::Transcribe`), Shift or not;
 * any other entry only with Shift (qa 2026-11-22_0107, E6). */
export const retranscribeTitleKey = (
  postProcessRequested: boolean,
  availability: PostProcessAvailability,
): string => {
  if (availability !== "ready") return "settings.history.retranscribe";
  return postProcessRequested
    ? "settings.history.retranscribeAgain"
    : "settings.history.retranscribeShift";
};

/** Re-transcribe with Shift also post-processes, when it can. */
export const retranscribeMode = (
  shiftKey: boolean,
  availability: PostProcessAvailability,
): ReprocessMode =>
  shiftKey && availability === "ready"
    ? "transcribe_and_post_process"
    : "transcribe";

/** DanyWhere (H6, strand E2): whether a prompt chosen by name in the
 * wand's menu can run: post-processing other than "never" and a provider
 * with a model; whether a prompt is active doesn't matter there. */
export const canRunChosenPrompt = (
  settings: Partial<PostProcessSettings> | null | undefined,
): boolean => {
  if (!settings || !postProcessingOn(settings)) return false;
  return chosenProviderWithModel(settings) !== null;
};
