/**
 * The app version a bundled release-note file belongs to, or null when the
 * file is not a release note for this app.
 *
 * The folder also holds Handy's notes (`<version>.md`, kept as upstream ships
 * them), named by Handy's versions. DanyWhere's own notes are named
 * `danywhere-<version>.md`, so only they appear in the "What's new" dialog.
 */
export const versionFromReleaseNotePath = (path: string): string | null => {
  const match = path.match(/release-notes\/danywhere-([^/]+)\.md$/);
  return match ? match[1] : null;
};
