// DanyWhere: context from earlier dictations in the same window
// (`dictation_context.rs`). Helpers for the settings and History.
import type { DictationContextSettings, EntryWindowContext } from "@/bindings";

/** The settings with every field filled in, as the backend defaults them. */
export type ContextSettings = Required<DictationContextSettings>;

export const DEFAULT_CONTEXT_SETTINGS: ContextSettings = {
  age_limit_minutes: 60,
  max_entries: 5,
  char_budget: 4000,
  excluded_apps: [],
};

/** Limits the backend accepts (`dictation_context.rs`). */
export const LIMITS = {
  age_limit_minutes: { min: 1, max: 24 * 60 },
  max_entries: { min: 1, max: 20 },
  char_budget: { min: 200, max: 20000 },
} as const;

/** The stored settings, with defaults for what an older store lacks. */
export const filled = (
  settings: DictationContextSettings | null | undefined,
): ContextSettings => ({
  ...DEFAULT_CONTEXT_SETTINGS,
  ...Object.fromEntries(
    Object.entries(settings ?? {}).filter(([, value]) => value !== undefined),
  ),
});

/** A whole number typed into a limit field, or null when it isn't one or
 * lies outside `range`. */
export const parseLimit = (
  draft: string,
  range: { min: number; max: number },
): number | null => {
  const trimmed = draft.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= range.min && value <= range.max ? value : null;
};

/** The exclusion list as typed: one app per line, blank lines dropped. */
export const parseExcludedApps = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

/** The window an entry was dictated in, as History shows it, or null. */
export const windowLabel = (
  context: EntryWindowContext | null | undefined,
): string | null => {
  const title = context?.window_title?.trim();
  if (!context || !title) return null;
  const app = context.window_app_name?.trim() || context.window_app_id || "";
  return app ? `${app} – ${title}` : title;
};

/** One window, as History filters by it: the criterion context is chosen by. */
export type WindowFilter = { app_id: string; title: string; label: string };

/** The filter for the window an entry was dictated in, or null. */
export const windowFilterOf = (
  context: EntryWindowContext | null | undefined,
): WindowFilter | null => {
  const label = windowLabel(context);
  const appId = context?.window_app_id;
  const title = context?.window_title;
  if (!label || !appId || !title) return null;
  return { app_id: appId, title, label };
};

/** Whether an entry was dictated in the filtered window. */
export const inWindow = (
  context: EntryWindowContext | null | undefined,
  filter: WindowFilter,
): boolean =>
  !!context &&
  context.window_app_id === filter.app_id &&
  context.window_title === filter.title;

/** The first `count` words of `text`, with "…" when there is more. */
export const firstWords = (text: string, count: number): string => {
  const words = text
    .trim()
    .split(/\s+/)
    .filter((word) => word.length > 0);
  const shown = words.slice(0, count).join(" ");
  return words.length > count ? `${shown} …` : shown;
};
