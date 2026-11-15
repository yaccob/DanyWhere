import type { AppSettings, ReprocessMode } from "@/bindings";

export type { ReprocessMode };

/** Whether post-processing can run with the current settings. */
export type PostProcessAvailability = "ready" | "off" | "not_set_up";

type PostProcessSettings = Pick<
  AppSettings,
  | "post_process_enabled"
  | "post_process_provider_id"
  | "post_process_providers"
  | "post_process_models"
  | "post_process_selected_prompt_id"
  | "post_process_prompts"
>;

/**
 * Mirrors what the backend checks before it calls the provider: switched
 * on, and a provider with a model and a selected prompt.
 */
export const postProcessAvailability = (
  settings: Partial<PostProcessSettings> | null | undefined,
): PostProcessAvailability => {
  if (!settings?.post_process_enabled) return "off";
  const providerId = settings.post_process_provider_id;
  const provider = settings.post_process_providers?.find(
    (p) => p.id === providerId,
  );
  const model = providerId ? settings.post_process_models?.[providerId] : "";
  const promptId = settings.post_process_selected_prompt_id;
  const prompt = settings.post_process_prompts?.find((p) => p.id === promptId);
  return provider && model?.trim() && prompt ? "ready" : "not_set_up";
};

/** Re-transcribe with Shift also post-processes, when it can. */
export const retranscribeMode = (
  shiftKey: boolean,
  availability: PostProcessAvailability,
): ReprocessMode =>
  shiftKey && availability === "ready"
    ? "transcribe_and_post_process"
    : "transcribe";
