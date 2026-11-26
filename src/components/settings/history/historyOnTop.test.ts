// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/historyOnTop.test.ts
// DanyWhere: History puts everything on top and never asks (user,
// 2026-11-19, H1–H5 in the sidecar's
// qa/2026-11-18_2027_verlauf-immer-oben-drauf.md). Post-processing any
// version, saving an edit of any version as new and transcribing again put
// their result on top, so nothing is deleted on the way and nothing asks
// first; versions are moved and deleted, the transcription too, by dragging
// a chip or from the current version's menu. ✨ has no menu of its own any
// more ("replace" and "redo from the previous version" went). The decisions
// themselves are tested in `versions.test.ts`; this checks that the
// components use them, and that dragging uses pointer events: Tauri's
// `dragDropEnabled` turns HTML5 drag and drop off on Windows.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = join(fileURLToPath(import.meta.url), "..");
const source = (file: string) => readFileSync(join(HERE, file), "utf8");
const LOCALE = join(HERE, "../../../i18n/locales/en/translation.json");

let checks = 0;
const failures: string[] = [];
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message}`);
  }
};

const has = (text: string, pattern: RegExp, file: string) =>
  assert.match(text, pattern, `${file} lacks ${pattern}`);
const hasNot = (text: string, pattern: RegExp, file: string) =>
  assert.doesNotMatch(text, pattern, `${file} still has ${pattern}`);

const FILES = [
  "HistorySettings.tsx",
  "VersionTrack.tsx",
  "PostProcessButton.tsx",
  "versions.ts",
];

check("nothing asks before a change of the versions", () => {
  assert.equal(existsSync(join(HERE, "confirmLoss.ts")), false);
  for (const file of FILES) hasNot(source(file), /confirmLoss/u, file);
});

check("nothing deletes versions on the way", () => {
  for (const file of FILES) {
    hasNot(source(file), /versionsLostBy|revertToVersion|goBack/u, file);
    hasNot(source(file), /retranscribeNeedsConfirm/u, file);
  }
});

check("✨ post-processes the version shown; its menu with one prompt", () => {
  const settings = source("HistorySettings.tsx");
  has(
    settings,
    /reprocess\(entry\.id, "post_process", shown, null, true\)/u,
    "HistorySettings.tsx",
  );
  // DanyWhere (strand Z1): with the entry's spoken instruction, as the
  // dictation was made; its menu also without it.
  has(
    settings,
    /reprocess\(entry\.id, "post_process", shown, null, false\)/u,
    "HistorySettings.tsx",
  );
  // H6 (strand E2): the menu lists every prompt by name; one chosen runs
  // alone on the version shown, its result on top.
  has(
    settings,
    /reprocess\(entry\.id, "post_process", shown, \[promptId\], true\)/u,
    "HistorySettings.tsx",
  );
  const button = source("PostProcessButton.tsx");
  has(button, /role="menu"/u, "PostProcessButton.tsx");
  has(button, /role="menuitemcheckbox"/u, "PostProcessButton.tsx");
  for (const file of FILES) {
    hasNot(source(file), /post_process_replace|post_process_redo/u, file);
  }
});

check("transcribing again names no version", () => {
  has(
    source("HistorySettings.tsx"),
    /reprocess\(entry\.id, mode, null, null, true\)/u,
    "HistorySettings.tsx",
  );
});

check("a chip is dragged with pointer events, not HTML5 drag and drop", () => {
  const track = source("VersionTrack.tsx");
  has(track, /onPointerDown=/u, "VersionTrack.tsx");
  has(track, /setPointerCapture\(/u, "VersionTrack.tsx");
  has(track, /dropSlot\(/u, "VersionTrack.tsx");
  hasNot(track, /draggable|onDragStart|onDrop=/u, "VersionTrack.tsx");
});

check("the current version has a menu to move and delete it", () => {
  const track = source("VersionTrack.tsx");
  has(track, /role="menu"/u, "VersionTrack.tsx");
  has(track, /versionActions\(/u, "VersionTrack.tsx");
  has(track, /onMove\(/u, "VersionTrack.tsx");
  has(track, /onDelete\(/u, "VersionTrack.tsx");
  const settings = source("HistorySettings.tsx");
  has(settings, /commands\.moveHistoryVersion/u, "HistorySettings.tsx");
  has(settings, /commands\.deleteHistoryVersion/u, "HistorySettings.tsx");
});

check(
  "History's strings name the new actions and none of the questions",
  () => {
    const versions = JSON.parse(readFileSync(LOCALE, "utf8")).settings.history
      .versions as Record<string, unknown>;
    for (const key of ["moveToTop", "moveUp", "moveDown", "deleteVersion"]) {
      assert.ok(key in versions, `en lacks settings.history.versions.${key}`);
    }
    const gone = Object.keys(versions).filter((key) =>
      /^(confirm|goBack|postProcessReplace|redo|moreActions)/u.test(key),
    );
    assert.deepEqual(gone, []);
  },
);

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAILED: ${failure}`);
  console.error(
    `historyOnTop: ${failures.length} of ${checks} checks failed, ${checks - failures.length} passed`,
  );
  process.exit(1);
}
console.log(`historyOnTop: all ${checks} checks passed`);
