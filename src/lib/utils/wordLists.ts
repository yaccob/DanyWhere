/** Helpers for the global word list and the additional word lists. */

export interface WordListLike {
  id: string;
  name: string;
  words?: string[];
}

/** Dropdown value for "no additional list, only the global one". Not empty:
 * the dropdown treats an empty value as "nothing selected". */
export const NO_WORD_LIST = "__none__";

/** Longest word or phrase the lists take. */
export const MAX_WORD_LENGTH = 50;

/** Strips characters that would break the model's prompt and collapses
 * whitespace, as the global list always did. */
export const normalizeWord = (word: string) =>
  word
    .replace(/[<>"']/g, "")
    .replace(/\s+/g, " ")
    .trim();

export type AddWordResult =
  | { kind: "added"; words: string[] }
  | { kind: "duplicate"; word: string }
  | { kind: "invalid" };

/** `words` with `input` appended, unless it's blank, too long or already in. */
export const addWord = (words: string[], input: string): AddWordResult => {
  const word = normalizeWord(input);
  if (!word || word.length > MAX_WORD_LENGTH) return { kind: "invalid" };
  if (words.includes(word)) return { kind: "duplicate", word };
  return { kind: "added", words: [...words, word] };
};

export const removeWord = (words: string[], word: string): string[] =>
  words.filter((existing) => existing !== word);

/** The active-list dropdown's value: the active list's id, or
 * {@link NO_WORD_LIST} when none is active or the id names no list. */
export const activeListOption = (
  lists: WordListLike[],
  activeId: string | null | undefined,
): string =>
  activeId && lists.some((list) => list.id === activeId)
    ? activeId
    : NO_WORD_LIST;
