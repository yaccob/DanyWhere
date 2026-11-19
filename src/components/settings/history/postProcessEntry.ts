import type { AppSettings, ReprocessMode } from "@/bindings";
import { hasActivePrompt } from "@/lib/promptLibrary";

export type { ReprocessMode };

/** Whether post-processing can run with the current settings. */
export type PostProcessAvailability = "ready" | "off" | "not_set_up";

type PostProcessSettings = Pick<
  AppSettings,
  | "post_process_enabled"
  | "post_process_provider_id"
  | "post_process_providers"
  | "post_process_models"
  | "post_process_prompts"
  | "post_process_prompt_order"
>;

/**
 * Mirrors what the backend checks before it calls the provider: switched
 * on, and a provider with a model and (DanyWhere, the prompt library) at
 * least one active prompt.
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
  const prompt = hasActivePrompt(
    settings.post_process_prompts,
    settings.post_process_prompt_order,
  );
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
