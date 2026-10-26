import type { ModelInfo } from "@/bindings";

/**
 * DanyWhere: the model onboarding downloads without being asked, so a fresh
 * install needs no decision before it can dictate — the first recommended
 * model (models arrive in rank order). Nothing is started when a model is
 * already on disk, a download is running, or nothing is recommended.
 */
export const autoDownloadCandidate = (
  models: ModelInfo[],
  downloading: Record<string, unknown>,
): string | null => {
  if (models.some((m) => m.is_downloaded)) return null;
  if (Object.keys(downloading).length > 0) return null;
  const first = models.find(
    (m) =>
      m.is_recommended && !(typeof m.source === "object" && "Url" in m.source),
  );
  return first?.id ?? null;
};

/**
 * The download starts while the user is still granting permissions; the model
 * step then takes it over (running, verifying or finished) so it selects the
 * model by itself. A failed or cancelled download is left to the user.
 */
export const modelToFollow = (
  started: string | null,
  models: ModelInfo[],
  downloading: Record<string, unknown>,
  verifying: Record<string, unknown>,
): string | null => {
  if (!started) return null;
  if (started in downloading || started in verifying) return started;
  return models.some((m) => m.id === started && m.is_downloaded)
    ? started
    : null;
};
