// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/overlayProfile.test.ts
import assert from "node:assert";
import { readFileSync } from "node:fs";
import {
  OVERLAY_WINDOW,
  PILL,
  contextGlyph,
  labelPill,
  profileGlyph,
  profileNoticeLayout,
  recordingLayout,
  recordingPill,
  workingLayout,
  workingPill,
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
  // DanyWhere: "Send Context" (2026-11-15).
  [
    "the notice after turning Send Context over shows the bubble, crossed out while off",
    () => {
      assert.strictEqual(contextGlyph(true, "notice"), "context");
      assert.strictEqual(contextGlyph(false, "notice"), "context-off");
    },
  ],
  [
    "a dictation's pill shows the bubble only while it sends context",
    () => {
      assert.strictEqual(contextGlyph(true, "dictation"), "context");
      assert.strictEqual(contextGlyph(false, "dictation"), null);
    },
  ],
  [
    "the bubble widens a pill with a profile by its own 20px",
    () => {
      const named = { named: true, glyph: true, context: true };
      assert.strictEqual(
        recordingPill(60, named).width - recordingLayout(60, true).width,
        20,
      );
      assert.strictEqual(
        workingPill(LABEL, 60, named).width -
          workingLayout(LABEL, 60, true).width,
        20,
      );
      assert.strictEqual(
        recordingPill(60, named, true).width -
          recordingLayout(60, true, true).width,
        20,
      );
    },
  ],
  [
    "with a single profile the bubble alone follows the separator",
    () => {
      const alone = { named: false, glyph: false, context: true };
      // 20px row padding, 44px side columns, 16px label padding, the
      // label, 8px separator, 20px bubble.
      assert.strictEqual(workingPill(150, 0, alone).width, 258);
      assert.strictEqual(
        recordingPill(500, alone).width,
        recordingPill(0, alone).width,
      );
      assert.strictEqual(recordingPill(0, alone).width, PILL.rest);
      // The Live pill: its own width, 8px separator, 20px bubble.
      assert.strictEqual(recordingPill(0, alone, true).width, PILL.live + 28);
    },
  ],
  [
    "the recording pill still cuts a name at 90px when the bubble follows it",
    () => {
      const named = { named: true, glyph: true, context: true };
      assert.strictEqual(recordingPill(200, named).nameMax, 90);
      assert.ok(fits(recordingPill(200, named).width, OVERLAY_WINDOW.session));
    },
  ],
  [
    "no pill with the bubble gets wider than its window",
    () => {
      for (let name = 0; name <= 600; name += 25) {
        for (const glyph of [true, false]) {
          const named = { named: true, glyph, context: true };
          assert.ok(
            fits(recordingPill(name, named).width, OVERLAY_WINDOW.session),
          );
          assert.ok(
            fits(workingPill(LABEL, name, named).width, OVERLAY_WINDOW.session),
          );
          assert.ok(
            fits(recordingPill(name, named, true).width, OVERLAY_WINDOW.live),
          );
          assert.ok(fits(workingPill(LABEL, name, named, true).width, 394));
        }
      }
    },
  ],
  // DanyWhere: the pills with the dot and one label (`LabelPill.tsx`): where
  // the text went, or which word list is on. A fixed 172px cut all but the
  // shortest of them (2026-11-17).
  [
    "a label pill is as wide as its label",
    () => {
      // 20px row padding, 44px side columns, 16px label padding, the label.
      assert.strictEqual(labelPill(200), 280);
      assert.strictEqual(labelPill(240) - labelPill(200), 40);
    },
  ],
  [
    "a short label keeps the resting pill's width",
    () => {
      assert.strictEqual(labelPill(40), PILL.rest);
      assert.strictEqual(labelPill(0), PILL.rest);
    },
  ],
  [
    "a label pill cuts its label only at its window's edge",
    () => {
      assert.strictEqual(labelPill(258), OVERLAY_WINDOW.notice - 2);
      assert.strictEqual(labelPill(1000), OVERLAY_WINDOW.notice - 2);
    },
  ],
  [
    "no label pill gets wider than its window",
    () => {
      for (let label = 0; label <= 600; label += 25) {
        assert.ok(fits(labelPill(label), OVERLAY_WINDOW.notice));
      }
    },
  ],
  [
    "the label pills show in overlay.rs's notice window",
    () => {
      const source = readFileSync(
        new URL("../../src-tauri/src/overlay.rs", import.meta.url),
        "utf8",
      );
      const body = source.match(/fn overlay_dimensions[\s\S]*?\n\}/)?.[0] ?? "";
      const states = body.match(/matches!\([\s\S]*?\)\s*\{/)?.[0] ?? "";
      for (const state of ["clipboard", "ready", "not_ready", "word_list"]) {
        assert.ok(states.includes(`"${state}"`), state);
      }
      assert.match(body, /\{\s*\(OVERLAY_NOTICE_WIDTH, OVERLAY_HEIGHT\)/);
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
