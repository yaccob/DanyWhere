import {
  groupModels,
  type CatalogModel,
  type MachineTier,
  type picksFor,
} from "./catalog";

type ModelPick = ReturnType<typeof picksFor>[number];

/**
 * DanyWhere offers fewer models than Handy, so none of the package's picks
 * may be on offer (e.g. for German), while onboarding's model step needs a
 * "Recommended" one. Then the best offered model for these languages, by
 * Handy's own ranking on this machine, becomes the pick (its other role, if
 * any, gives way); with no model that speaks all of them, the best partial
 * match. Nothing offered (yet): the picks stay as they are.
 */
export const withFallbackPick = (
  picks: ModelPick[],
  languages: string[],
  tier: MachineTier,
  offered: CatalogModel[],
): ModelPick[] => {
  if (picks.some((p) => p.role === "pick")) return picks;
  // Without roles, every offered model is ranked in `others` or `partial`.
  const { others, partial } = groupModels(languages, tier, offered, []);
  const best = (others[0] ?? partial[0])?.model;
  if (!best) return picks;
  return [
    { model: best, role: "pick" },
    ...picks.filter((p) => p.model.id !== best.id),
  ];
};
