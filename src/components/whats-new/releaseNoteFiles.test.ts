// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/whats-new/releaseNoteFiles.test.ts
import assert from "node:assert";
import { versionFromReleaseNotePath } from "./releaseNoteFiles";

const dir = "../../content/release-notes";

// Handy's release notes (named by Handy's version) are never shown as
// DanyWhere news, whatever DanyWhere's own version is.
assert.strictEqual(versionFromReleaseNotePath(`${dir}/0.9.7.md`), null);
assert.strictEqual(versionFromReleaseNotePath(`${dir}/0.1.0.md`), null);
assert.strictEqual(versionFromReleaseNotePath(`${dir}/README.md`), null);

// DanyWhere's own notes are named danywhere-<version>.md.
assert.strictEqual(
  versionFromReleaseNotePath(`${dir}/danywhere-0.1.1.md`),
  "0.1.1",
);
assert.strictEqual(
  versionFromReleaseNotePath(`${dir}/danywhere-1.0.0.md`),
  "1.0.0",
);

console.log("release note files: all checks passed");
