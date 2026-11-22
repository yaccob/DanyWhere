import { BookA, Languages, Radio as LiveIcon } from "lucide-react";

/**
 * What a model can do besides transcribe: its icon, and its flag on a table
 * model and in the catalog's `capabilities`. Words under `modelTable.tags`.
 */
export const CAPABILITIES = {
  preview: { Icon: LiveIcon, flag: "streaming" },
  translate: { Icon: Languages, flag: "translate" },
  // DanyWhere: takes the word lists as vocabulary while it transcribes.
  wordLists: { Icon: BookA, flag: "vocabulary" },
} as const;

export type Capability = keyof typeof CAPABILITIES;

export const CAPABILITY_KEYS = Object.keys(CAPABILITIES) as Capability[];
