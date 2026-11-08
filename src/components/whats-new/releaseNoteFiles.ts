/**
 * Which DanyWhere release a bundled release-note file belongs to, and in
 * which language it is written.
 *
 * The folder also holds Handy's notes (`<version>.md`, kept as upstream ships
 * them), named by Handy's versions; they are never DanyWhere's news. DanyWhere's
 * own notes are `danywhere-<version>.md` in English, the fallback for every
 * language, plus optional translations `danywhere-<version>.<language>.md`
 * named by the app's language codes (`de`, `zh-TW`, ...).
 */
export interface ReleaseNoteFile {
  version: string;
  /** The translation's language code, or null for the English file. */
  language: string | null;
}

const RELEASE_NOTE_FILE =
  /release-notes\/danywhere-(\d+\.\d+\.\d+)(?:\.([A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})?))?\.md$/;

export const parseReleaseNotePath = (path: string): ReleaseNoteFile | null => {
  const match = path.match(RELEASE_NOTE_FILE);
  return match ? { version: match[1], language: match[2] ?? null } : null;
};

/**
 * One version's note for the app's language: the translation for exactly that
 * language, else for its base language (`zh-TW` → `zh`, `de-AT` → `de`), else
 * the English file (key ""). Null when the version has no English file and no
 * matching translation.
 */
export const pickReleaseNoteMarkdown = (
  byLanguage: Readonly<Record<string, string>>,
  language: string,
): string | null => {
  const wanted = language.trim().toLowerCase();
  const find = (code: string) =>
    Object.entries(byLanguage).find(
      ([key]) => key !== "" && key.toLowerCase() === code,
    )?.[1];

  if (wanted) {
    const exact = find(wanted);
    if (exact !== undefined) return exact;
    const base = wanted.split("-")[0];
    const baseNote = base !== wanted ? find(base) : undefined;
    if (baseNote !== undefined) return baseNote;
  }
  return byLanguage[""] ?? null;
};
