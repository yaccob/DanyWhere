// DanyWhere: what a user sees of a rendered page, and what must never be
// among it (qa 2026-11-22_0107, errors 1 to 3): a placeholder the code
// didn't fill ("Items: {{number}}"), a translation key instead of its text,
// an internal id instead of a name ("end_1795307930035"), lines under a
// setting that isn't there. happy-dom applies no stylesheet, so hidden
// means what the app's markup says: Tailwind's `hidden`, the `hidden`
// attribute, `aria-hidden`.

const isHidden = (element: Element): boolean =>
  element.classList.contains("hidden") ||
  element.hasAttribute("hidden") ||
  element.getAttribute("aria-hidden") === "true";

const ATTRIBUTES = ["title", "aria-label", "placeholder"] as const;

const collect = (
  node: Node,
  texts: string[],
  withAttributes: boolean,
): void => {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent?.trim();
    if (text) texts.push(text);
    return;
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return;
  const element = node as Element;
  if (isHidden(element)) return;
  if (element.tagName === "SCRIPT" || element.tagName === "STYLE") return;
  if (withAttributes) {
    for (const name of ATTRIBUTES) {
      const value = element.getAttribute(name)?.trim();
      if (value) texts.push(value);
    }
    if (element instanceof HTMLInputElement && element.type !== "checkbox") {
      const value = element.value.trim();
      if (value) texts.push(value);
    }
  }
  for (const child of Array.from(element.childNodes)) {
    collect(child, texts, withAttributes);
  }
};

/** Whether `element` and every element around it show. testing-library's
 * queries can't tell, since happy-dom applies no stylesheet: filter what
 * they find with this. */
export const isVisible = (element: Element): boolean => {
  for (let node: Element | null = element; node; node = node.parentElement) {
    if (isHidden(node)) return false;
  }
  return true;
};

/** The visible text of `root`, piece by piece, without tooltips. */
export const visibleTexts = (root: Node = document.body): string[] => {
  const texts: string[] = [];
  collect(root, texts, false);
  return texts;
};

/** The visible text of `root` as one line, pieces joined by spaces. */
export const visibleText = (root: Node = document.body): string =>
  visibleTexts(root).join(" ").replace(/\s+/g, " ");

/** Everything a user can read of `root`: its visible text, the tooltips
 * (`title`), the screen reader's labels and the fields' contents. */
export const readableTexts = (root: Node = document.body): string[] => {
  const texts: string[] = [];
  collect(root, texts, true);
  return texts;
};

/** The binding ids of the shortcuts, and the ids of the start, hold and
 * instruction shortcuts out of the box, which a page names, never shows. */
const BINDING_IDS = [
  "transcribe_with_post_process",
  "paste_last_transcript",
  "copy_last_transcript",
  "cycle_word_list",
  "toggle_profile_auto",
  "hold_direct",
  "hold_post",
  "instruction_tab",
];

const PROBLEMS: { what: string; pattern: RegExp }[] = [
  { what: "unfilled placeholder", pattern: /\{\{[^}]*\}\}/ },
  {
    what: "translation key",
    pattern:
      /\b(?:settings|common|tray|errors|onboarding|accessibility|footer|sidebar)\.[A-Za-z0-9_]+\.[A-Za-z0-9_.]+/,
  },
  { what: "profile id", pattern: /\bprofile_\d+/ },
  { what: "end shortcut id", pattern: /\bend_\d{6,}/ },
  {
    what: "start, hold or instruction shortcut id",
    pattern: /\b(?:start|hold|instruction)_\d{6,}/,
  },
  {
    what: "shortcut reference",
    pattern: /\b(?:start|hold|instruction|end)_key(?::|\b)/,
  },
  {
    what: "list field",
    pattern: /\b(?:start|hold|instruction|end)_keys\b/,
  },
  { what: "word list id", pattern: /\bword_list_\d+/ },
  {
    what: "post-processing mode",
    pattern: /\b(?:post_process_mode|with_instruction_key)\b/,
  },
  {
    what: "binding id",
    pattern: new RegExp(`\\b(?:${BINDING_IDS.join("|")})\\b`),
  },
  // The codes of `SignInFailure` that are no word; the others ("denied",
  // "refused" …) can be part of a sentence.
  {
    what: "sign-in failure code",
    pattern: /\b(?:timed_out|not_eligible|no_free_model)\b/,
  },
];

/** Each readable text of `root` that holds something a user must never
 * read, as "<what>: <text>"; empty when the page is clean. */
export const rawTextIn = (root: Node = document.body): string[] => {
  const found: string[] = [];
  for (const text of readableTexts(root)) {
    for (const { what, pattern } of PROBLEMS) {
      if (pattern.test(text)) found.push(`${what}: ${text}`);
    }
  }
  return found;
};
