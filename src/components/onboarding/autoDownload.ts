import type { DownloadState } from "@/lib/models/downloadState";

export interface AutoDownloadInput {
  /** The model step's picks for the chosen languages (catalog ids). */
  picks: { model: { id: string }; role: string }[];
  /** Where each offered model's download is. */
  states: DownloadState["state"][];
  /** A model the onboarding already has: picked earlier, or found on disk. */
  chosen: string | null;
}

/**
 * DanyWhere: the model a fresh install starts downloading as soon as the
 * spoken languages are chosen, so it loads while permissions are granted —
 * the "Recommended" pick the model step will preselect. Nothing starts when
 * the onboarding already has a model, an offered model is on disk, or a
 * download is running or being checked. A failed earlier attempt does not
 * block a new start.
 */
export const autoDownloadPick = ({
  picks,
  states,
  chosen,
}: AutoDownloadInput): string | null => {
  if (chosen) return null;
  if (
    states.some((s) => s === "ready" || s === "downloading" || s === "checking")
  )
    return null;
  return picks.find((p) => p.role === "pick")?.model.id ?? null;
};
