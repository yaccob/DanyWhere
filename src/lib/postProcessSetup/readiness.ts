// DanyWhere: whether post-processing can run with a scope's settings, as the
// backend checks it before it sends anything (`post_process_ready` in
// src-tauri/src/post_process_setup.rs). The defaults' readiness comes from
// `get_post_process_setup_status`; a profile's page has only its settings.
import type { AppSettings, Readiness } from "@/bindings";

/** The providers that need no key: a server of one's own and Apple
 * Intelligence; ChatGPT needs its sign-in instead (`needs_key`). */
const WITHOUT_KEY = new Set(["custom", "apple_intelligence", "chatgpt"]);

type ReadinessSettings = Partial<
  Pick<
    AppSettings,
    | "post_process_provider_id"
    | "post_process_providers"
    | "post_process_models"
    | "post_process_api_keys"
    | "chatgpt_signed_in"
  >
>;

/** What post-processing lacks with `settings` (the values of the scope
 * shown), or "ready". */
export const postProcessReadiness = (
  settings: ReadinessSettings | null | undefined,
): Readiness => {
  const id = settings?.post_process_provider_id;
  const provider = settings?.post_process_providers?.find((p) => p.id === id);
  if (!id || !provider) return "provider";
  if (!settings?.post_process_models?.[id]?.trim()) return "model";
  if (id === "chatgpt") {
    return settings.chatgpt_signed_in ? "ready" : "sign_in";
  }
  if (!WITHOUT_KEY.has(id) && !settings.post_process_api_keys?.[id]?.trim()) {
    return "key";
  }
  return "ready";
};
