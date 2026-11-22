// DanyWhere: the prompt library (src-tauri/src/prompt_library.rs, idea 1).
// All profiles share one library of post-processing prompts; a profile keeps
// which of them are active and the order of the whole list. These functions
// work on that order for the list editor and History; they never change
// their arguments.
import type { LLMPrompt, PromptOrderEntry } from "@/bindings";

export type { PromptOrderEntry };
export type LibraryPrompt = Pick<LLMPrompt, "id" | "name" | "prompt">;

/** Where a prompt from before the fixed frame wants the transcript. */
const OUTPUT_PLACEHOLDER = "${output}";

/** One prompt of the list as the editor shows it. */
export interface PromptRow {
  prompt: LibraryPrompt;
  active: boolean;
  /** 1, 2, 3 … on active prompts only, in their order. */
  number: number | null;
}

/**
 * `order` covering the whole library once: its entries for prompts the
 * library has, first occurrence only, then the library's other prompts,
 * inactive, in the library's order (as the backend's `normalized`).
 */
export const normalizedOrder = (
  prompts: LibraryPrompt[],
  order: PromptOrderEntry[],
): PromptOrderEntry[] => {
  const known = new Set(prompts.map((prompt) => prompt.id));
  const seen = new Set<string>();
  const result: PromptOrderEntry[] = [];
  for (const entry of order) {
    if (known.has(entry.id) && !seen.has(entry.id)) {
      seen.add(entry.id);
      result.push({ id: entry.id, active: entry.active });
    }
  }
  for (const prompt of prompts) {
    if (!seen.has(prompt.id)) result.push({ id: prompt.id, active: false });
  }
  return result;
};

/** The library in the profile's order, numbered. */
export const promptRows = (
  prompts: LibraryPrompt[],
  order: PromptOrderEntry[],
): PromptRow[] => {
  const byId = new Map(prompts.map((prompt) => [prompt.id, prompt]));
  let count = 0;
  return normalizedOrder(prompts, order).map((entry) => {
    const prompt = byId.get(entry.id) as LibraryPrompt;
    const number = entry.active ? ++count : null;
    return { prompt, active: entry.active, number };
  });
};

/** `order` with prompt `id` (in)active. */
export const withActive = (
  order: PromptOrderEntry[],
  id: string,
  active: boolean,
): PromptOrderEntry[] =>
  order.map((entry) => (entry.id === id ? { ...entry, active } : entry));

/** `order` with the entry at `from` moved to `to` (clamped to the list). */
export const moved = (
  order: PromptOrderEntry[],
  from: number,
  to: number,
): PromptOrderEntry[] => {
  if (from < 0 || from >= order.length) return order;
  const target = Math.max(0, Math.min(order.length - 1, to));
  if (target === from) return order;
  const result = [...order];
  const [entry] = result.splice(from, 1);
  result.splice(target, 0, entry);
  return result;
};

/** `order` with prompt `id` moved up (`-1`) or down (`1`); unchanged at the
 * ends and for an unknown id. */
export const movedBy = (
  order: PromptOrderEntry[],
  id: string,
  delta: number,
): PromptOrderEntry[] => {
  const from = order.findIndex((entry) => entry.id === id);
  if (from < 0) return order;
  const to = from + delta;
  if (to < 0 || to >= order.length) return order;
  return moved(order, from, to);
};

/**
 * Where the entry dragged from `from` lands when the pointer is at
 * `pointerY`: the number of other entries whose middle (`midpoints`, as
 * measured when the drag began) lies above the pointer.
 */
export const dropIndex = (
  pointerY: number,
  midpoints: number[],
  from: number,
): number =>
  midpoints.filter((middle, index) => index !== from && middle < pointerY)
    .length;

/** The active prompts with a text, in the profile's order. */
const activePrompts = (
  prompts: LibraryPrompt[],
  order: PromptOrderEntry[],
): LibraryPrompt[] => {
  const byId = new Map(prompts.map((prompt) => [prompt.id, prompt]));
  const seen = new Set<string>();
  const result: LibraryPrompt[] = [];
  for (const entry of order) {
    const prompt = byId.get(entry.id);
    if (entry.active && prompt?.prompt.trim() && !seen.has(entry.id)) {
      seen.add(entry.id);
      result.push(prompt);
    }
  }
  return result;
};

/** Whether post-processing has a prompt to run: at least one active prompt
 * with a text (the backend's `request_prompts` is not empty). */
export const hasActivePrompt = (
  prompts: LibraryPrompt[] | null | undefined,
  order: PromptOrderEntry[] | null | undefined,
): boolean => activePrompts(prompts ?? [], order ?? []).length > 0;

/** Whether another prompt than `exceptId` is called `name` (trimmed, as the
 * backend compares). */
export const isNameTaken = (
  prompts: LibraryPrompt[],
  name: string,
  exceptId?: string,
): boolean =>
  prompts.some(
    (prompt) => prompt.id !== exceptId && prompt.name.trim() === name.trim(),
  );

/**
 * Whether prompt `id` runs alone and leaves the other active prompts out:
 * it is the first active one with `${output}`, which can't be combined, and
 * others are active too.
 */
export const runsAlone = (
  prompts: LibraryPrompt[],
  order: PromptOrderEntry[],
  id: string,
): boolean => {
  const active = activePrompts(prompts, order);
  const first = active.find((prompt) =>
    prompt.prompt.includes(OUTPUT_PLACEHOLDER),
  );
  return first?.id === id && active.length > 1;
};
