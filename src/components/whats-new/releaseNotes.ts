import {
  parseReleaseNotePath,
  pickReleaseNoteMarkdown,
} from "./releaseNoteFiles";

export interface ReleaseNote {
  version: string;
  markdown: string;
}

interface FindReleaseNoteOptions {
  currentVersion: string;
  lastSeenVersion: string;
  /** The app's interface language; picks the note's translation. */
  language: string;
}

const releaseNoteModules = import.meta.glob<string>(
  "../../content/release-notes/*.md",
  {
    eager: true,
    import: "default",
    query: "?raw",
  },
);

/** Each version's notes by language ("" is the English file). */
const releaseNotesByVersion = new Map<string, Record<string, string>>();

const parseVersion = (version: string): [number, number, number] | null => {
  const normalized = version.trim().replace(/^v/i, "");
  const match = normalized.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;

  return [Number(match[1]), Number(match[2]), Number(match[3])];
};

for (const [path, markdown] of Object.entries(releaseNoteModules)) {
  const file = parseReleaseNotePath(path);
  if (!file) continue;

  if (!parseVersion(file.version)) continue;

  const byLanguage = releaseNotesByVersion.get(file.version) ?? {};
  byLanguage[file.language ?? ""] = markdown;
  releaseNotesByVersion.set(file.version, byLanguage);
}

const compareVersions = (a: string, b: string) => {
  const parsedA = parseVersion(a);
  const parsedB = parseVersion(b);

  if (!parsedA || !parsedB) return 0;

  for (let index = 0; index < parsedA.length; index += 1) {
    if (parsedA[index] !== parsedB[index]) {
      return parsedA[index] - parsedB[index];
    }
  }

  return 0;
};

const noteFor = (version: string, language: string): ReleaseNote | null => {
  const byLanguage = releaseNotesByVersion.get(version);
  const markdown = byLanguage
    ? pickReleaseNoteMarkdown(byLanguage, language)
    : null;
  return markdown === null ? null : { version, markdown };
};

export const findReleaseNoteToShow = ({
  currentVersion,
  lastSeenVersion,
  language,
}: FindReleaseNoteOptions): ReleaseNote | null => {
  if (!parseVersion(currentVersion)) return null;

  const hasValidLastSeenVersion = parseVersion(lastSeenVersion) !== null;
  const candidate = Array.from(releaseNotesByVersion.keys())
    .filter((version) => compareVersions(version, currentVersion) <= 0)
    .filter(
      (version) =>
        !hasValidLastSeenVersion ||
        compareVersions(version, lastSeenVersion) > 0,
    )
    .sort((a, b) => compareVersions(b, a))[0];

  if (!candidate) return null;

  return noteFor(candidate, language);
};

export const findLatestReleaseNote = (language: string): ReleaseNote | null => {
  const candidate = Array.from(releaseNotesByVersion.keys()).sort((a, b) =>
    compareVersions(b, a),
  )[0];

  if (!candidate) return null;

  return noteFor(candidate, language);
};
