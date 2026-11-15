// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/overlayProfile.test.ts
import assert from "node:assert";
import { readFileSync } from "node:fs";
import {
  OVERLAY_WINDOW,
  PILL,
  profileGlyph,
  profileNoticeLayout,
  recordingLayout,
  workingLayout,
} from "./overlayProfile";

// A card's content box is its window's width less its 1px border on each side.
const fits = (width: number, window: number) => width <= window - 2;

// About the width of "Transcribing..." / "Transkribiere..." in 12px text.
const LABEL = 88;

const rustConst = (name: string) => {
  const source = readFileSync(
    new URL("../../src-tauri/src/overlay.rs", import.meta.url),
    "utf8",
  );
  const match = source.match(new RegExp(`const ${name}: f64 = ([0-9.]+);`));
  return match ? Number(match[1]) : NaN;
};

const cssVar = (name: string) => {
  const source = readFileSync(
    new URL("../overlay/RecordingOverlay.css", import.meta.url),
    "utf8",
  );
  const match = source.match(new RegExp(`${name}: ([0-9]+)px;`));
  return match ? Number(match[1]) : NaN;
};

const cases: Array<[string, () => void]> = [
  [
    "a notice shows the window while choosing by application is on, the padlock while it is off, the dot without assignments",
    () => {
      assert.strictEqual(profileGlyph("auto", "notice"), "window");
      assert.strictEqual(profileGlyph("lock", "notice"), "lock");
      assert.strictEqual(profileGlyph("none", "notice"), "dot");
    },
  ],
  [
    "a dictation's pill shows the same glyphs, and none without assignments",
    () => {
      assert.strictEqual(profileGlyph("auto", "dictation"), "window");
      assert.strictEqual(profileGlyph("lock", "dictation"), "lock");
      assert.strictEqual(profileGlyph("none", "dictation"), null);
    },
  ],
  [
    "the notice is as wide as its glyph and name",
    () => {
      const short = profileNoticeLayout(40);
      const long = profileNoticeLayout(120);
      assert.strictEqual(long.width - short.width, 80);
      assert.ok(short.width < PILL.rest, `${short.width}`);
      assert.ok(short.nameMax >= 40);
    },
  ],
  [
    "the notice cuts a long name only at its window's edge",
    () => {
      const layout = profileNoticeLayout(1000);
      assert.strictEqual(layout.width, OVERLAY_WINDOW.notice - 2);
      assert.ok(layout.nameMax > 250, `${layout.nameMax}`);
    },
  ],
  [
    "the recording pill shows a name of up to 90px and cuts it after that",
    () => {
      assert.strictEqual(recordingLayout(200, true).nameMax, 90);
      assert.strictEqual(recordingLayout(200, false).nameMax, 90);
      assert.ok(fits(recordingLayout(200, true).width, OVERLAY_WINDOW.session));
    },
  ],
  [
    "the recording pill grows with the name and its glyph",
    () => {
      const plain = recordingLayout(60, false).width;
      const glyph = recordingLayout(60, true).width;
      assert.strictEqual(recordingLayout(80, true).width - glyph, 20);
      assert.strictEqual(glyph - plain, 19);
    },
  ],
  [
    "a recording pill with a short name is no narrower than without a profile",
    () => assert.strictEqual(recordingLayout(5, false).width, PILL.rest),
  ],
  [
    "the working pill fits the transcribing label and the recording pill's longest name",
    () => {
      const layout = workingLayout(LABEL, 90, true);
      assert.ok(layout.nameMax >= 90, `${layout.nameMax}`);
      assert.ok(fits(layout.width, OVERLAY_WINDOW.session), `${layout.width}`);
      assert.ok(layout.width > PILL.work, `${layout.width}`);
    },
  ],
  [
    "the working pill cuts a long name only at its window's edge",
    () => {
      const layout = workingLayout(LABEL, 1000, true);
      assert.strictEqual(layout.width, OVERLAY_WINDOW.session - 2);
      assert.ok(layout.nameMax > 90, `${layout.nameMax}`);
    },
  ],
  [
    "the Live pill grows by the profile, its panel keeps its width",
    () => {
      const live = recordingLayout(60, true, true);
      assert.strictEqual(live.width, PILL.live + 33 + 60);
      assert.ok(fits(live.width, OVERLAY_WINDOW.live));
      assert.ok(fits(workingLayout(LABEL, 1000, true, true).width, 394));
    },
  ],
  [
    "no pill gets wider than its window",
    () => {
      for (let name = 0; name <= 600; name += 25) {
        for (const glyph of [true, false]) {
          assert.ok(
            fits(profileNoticeLayout(name).width, OVERLAY_WINDOW.notice),
          );
          assert.ok(
            fits(recordingLayout(name, glyph).width, OVERLAY_WINDOW.session),
          );
          assert.ok(
            fits(
              workingLayout(LABEL, name, glyph).width,
              OVERLAY_WINDOW.session,
            ),
          );
          assert.ok(
            fits(recordingLayout(name, glyph, true).width, OVERLAY_WINDOW.live),
          );
        }
      }
    },
  ],
  [
    "the window widths are overlay.rs's",
    () => {
      assert.strictEqual(OVERLAY_WINDOW.session, rustConst("OVERLAY_WIDTH"));
      assert.strictEqual(
        OVERLAY_WINDOW.notice,
        rustConst("OVERLAY_NOTICE_WIDTH"),
      );
      assert.strictEqual(
        OVERLAY_WINDOW.live,
        rustConst("OVERLAY_STREAM_WIDTH"),
      );
    },
  ],
  [
    "the fixed pill widths are RecordingOverlay.css's",
    () => {
      assert.strictEqual(PILL.rest, cssVar("--ov-rest-w"));
      assert.strictEqual(PILL.live, cssVar("--ov-pill-w"));
      assert.strictEqual(PILL.work, cssVar("--ov-work-w"));
    },
  ],
];

let failed = 0;
for (const [name, check] of cases) {
  try {
    check();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(`overlayProfile: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`overlayProfile: all ${cases.length} checks passed`);
