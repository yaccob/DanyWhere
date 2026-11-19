// DanyWhere: the menu of History's ✨ (H6, qa 2026-11-18_2027): every
// prompt of the shared library by name, in the order of the profile in use
// (whose prompts a click on the wand itself runs), the active ones checked.
// Choosing one post-processes the version shown with that prompt alone.
import {
  normalizedOrder,
  type LibraryPrompt,
  type PromptOrderEntry,
} from "@/lib/promptLibrary";

/** Characters a name keeps in the menu, "…" included. */
export const MENU_NAME_LENGTH = 32;

/** Lines of a prompt's text its tooltip shows. */
const TOOLTIP_LINES = 3;
/** Characters a line of the tooltip keeps. */
const TOOLTIP_LINE_LENGTH = 90;

export interface PromptMenuItem {
  id: string;
  /** The name, cut to fit the menu. */
  label: string;
  /** The whole name and the first lines of the text. */
  tooltip: string;
  /** Active in the profile in use. */
  active: boolean;
}

/** `text` cut to `length` characters (not UTF-16 units), "…" included. */
const cutTo = (text: string, length: number): string => {
  const chars = [...text];
  if (chars.length <= length) return text;
  return `${chars
    .slice(0, length - 1)
    .join("")
    .trimEnd()}…`;
};

/** A name as the menu shows it: long ones cut with "…". */
export const shortName = (name: string): string => {
  const chars = [...name];
  if (chars.length <= MENU_NAME_LENGTH) return name;
  return `${chars.slice(0, MENU_NAME_LENGTH - 1).join("")}…`;
};

/** The whole name, then the first non-empty lines of the text, "…" when
 * there is more. */
export const promptTooltip = (
  prompt: LibraryPrompt,
  lines = TOOLTIP_LINES,
): string => {
  const textLines = prompt.prompt
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  if (textLines.length === 0) return prompt.name;
  const shown = textLines
    .slice(0, lines)
    .map((line) => cutTo(line, TOOLTIP_LINE_LENGTH));
  const more = textLines.length > lines ? "\n…" : "";
  return `${prompt.name}\n\n${shown.join("\n")}${more}`;
};

/** Every prompt of the library in `order` (the profile in use's), the
 * library's others after it; prompts that are gone are left out. */
export const promptMenuItems = (
  prompts: LibraryPrompt[],
  order: PromptOrderEntry[],
): PromptMenuItem[] => {
  const byId = new Map(prompts.map((prompt) => [prompt.id, prompt]));
  return normalizedOrder(prompts, order).flatMap((entry) => {
    const prompt = byId.get(entry.id);
    if (!prompt) return [];
    return [
      {
        id: prompt.id,
        label: shortName(prompt.name),
        tooltip: promptTooltip(prompt),
        active: entry.active,
      },
    ];
  });
};
