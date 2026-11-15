// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/historyBox.test.ts
import assert from "node:assert/strict";
import {
  CLICK_SLOP_PX,
  DEFAULT_BOX_LINES,
  FALLBACK_METRICS,
  MAX_BOX_HEIGHT_PX,
  MIN_BOX_LINES,
  activatesEditing,
  boxHeightSetting,
  boxKind,
  boxMetrics,
  boxSize,
  clampBoxHeight,
  dragHeight,
  editBoxHeight,
  heightOnRelease,
  linesHeight,
  maxBoxHeight,
  minBoxHeight,
  scrollToShow,
  storedBoxHeight,
} from "./historyBox";

let checks = 0;
let failures = 0;
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

// 20 px lines, 6 px padding and a 1 px border at top and bottom.
const metrics = { lineHeight: 20, chrome: 14 };

check("the newest entry shows 15 lines, the others 4", () => {
  assert.equal(DEFAULT_BOX_LINES.newest, 15);
  assert.equal(DEFAULT_BOX_LINES.other, 4);
  assert.equal(MIN_BOX_LINES, 2);
});

check("the first entry of the list is the newest kind", () => {
  assert.equal(boxKind(true), "newest");
  assert.equal(boxKind(false), "other");
});

check("each kind has its own setting", () => {
  assert.equal(boxHeightSetting("newest"), "history_box_height_newest");
  assert.equal(boxHeightSetting("other"), "history_box_height_other");
});

check("the stored height is read per kind; missing = none", () => {
  const settings = {
    history_box_height_newest: 320,
    history_box_height_other: null,
  };
  assert.equal(storedBoxHeight(settings, "newest"), 320);
  assert.equal(storedBoxHeight(settings, "other"), null);
  assert.equal(storedBoxHeight({}, "newest"), null);
  assert.equal(storedBoxHeight(null, "other"), null);
});

check("a box of n lines is n line heights plus padding and border", () => {
  assert.equal(linesHeight(4, metrics), 94);
  assert.equal(linesHeight(15, metrics), 314);
  assert.equal(minBoxHeight(metrics), 54);
});

check("without a dragged height a kind grows to its default lines", () => {
  assert.equal(maxBoxHeight("newest", null, metrics), 314);
  assert.equal(maxBoxHeight("other", null, metrics), 94);
  assert.equal(maxBoxHeight("other", undefined, metrics), 94);
});

check("a dragged height is the maximum of its kind", () => {
  assert.equal(maxBoxHeight("other", 200, metrics), 200);
  assert.equal(maxBoxHeight("newest", 120, metrics), 120);
});

check("a stored height below two lines or above the limit is clamped", () => {
  assert.equal(maxBoxHeight("other", 10, metrics), 54);
  assert.equal(maxBoxHeight("newest", 999_999, metrics), MAX_BOX_HEIGHT_PX);
  assert.equal(clampBoxHeight(54.4, metrics), 54);
  assert.equal(clampBoxHeight(130.6, metrics), 131);
});

check("dragging changes the height by the distance moved", () => {
  assert.equal(dragHeight(94, 500, 560, metrics), 154);
  assert.equal(dragHeight(154, 500, 470, metrics), 124);
});

check("dragging never makes a box smaller than two lines", () => {
  assert.equal(dragHeight(94, 500, 300, metrics), 54);
  assert.equal(dragHeight(94, 0, 100_000, metrics), MAX_BOX_HEIGHT_PX);
});

check("a release stores the dragged height only after a move", () => {
  assert.equal(heightOnRelease(500, 560, 154), 154);
  assert.equal(heightOnRelease(500, 499, 93), 93);
  // A click or a double-click on the grip moves nothing and stores nothing.
  assert.equal(heightOnRelease(500, 500, 94), null);
});

check("the edit field starts at the box's height, at least two lines", () => {
  assert.equal(editBoxHeight(94, metrics), 94);
  assert.equal(editBoxHeight(34, metrics), 54);
  assert.equal(editBoxHeight(200.4, metrics), 200);
});

check("a shown box is as tall as its text up to the maximum", () => {
  assert.deepEqual(
    boxSize({ editing: false, dragged: null, maxHeight: 94, editHeight: 0 }),
    { maxHeight: 94 },
  );
});

check("a dragged box is exactly the dragged height, text or not", () => {
  assert.deepEqual(
    boxSize({ editing: false, dragged: 300, maxHeight: 94, editHeight: 0 }),
    { height: 300 },
  );
  assert.deepEqual(
    boxSize({ editing: true, dragged: 70, maxHeight: 94, editHeight: 94 }),
    { height: 70 },
  );
});

check("an edited box keeps its height while the text changes", () => {
  assert.deepEqual(
    boxSize({ editing: true, dragged: null, maxHeight: 314, editHeight: 94 }),
    { height: 94 },
  );
});

check("a plain click on the text starts editing", () => {
  const click = { button: 0, collapsed: true, distance: 0, onScrollbar: false };
  assert.equal(activatesEditing(click), true);
  assert.equal(activatesEditing({ ...click, distance: CLICK_SLOP_PX }), true);
});

check("selecting text, dragging, the scroll bar or other buttons don't", () => {
  const click = { button: 0, collapsed: true, distance: 0, onScrollbar: false };
  assert.equal(activatesEditing({ ...click, collapsed: false }), false);
  assert.equal(
    activatesEditing({ ...click, distance: CLICK_SLOP_PX + 1 }),
    false,
  );
  assert.equal(activatesEditing({ ...click, onScrollbar: true }), false);
  assert.equal(activatesEditing({ ...click, button: 2 }), false);
});

check("a hidden match is scrolled into the middle of the box", () => {
  // Box shows 0..80; a match at 200..220 is below it.
  assert.equal(
    scrollToShow({ top: 200, height: 20, scrollTop: 0, clientHeight: 80 }),
    170,
  );
  // Above the shown part.
  assert.equal(
    scrollToShow({ top: 10, height: 20, scrollTop: 100, clientHeight: 80 }),
    0,
  );
});

check("a match already in view leaves the box where it is", () => {
  assert.equal(
    scrollToShow({ top: 30, height: 20, scrollTop: 0, clientHeight: 80 }),
    null,
  );
  assert.equal(
    scrollToShow({ top: 140, height: 20, scrollTop: 100, clientHeight: 80 }),
    null,
  );
});

check("metrics come from the box's computed style", () => {
  assert.deepEqual(
    boxMetrics({
      lineHeight: "20px",
      fontSize: "14px",
      paddingTop: "6px",
      paddingBottom: "6px",
      borderTopWidth: "1px",
      borderBottomWidth: "1px",
    }),
    { lineHeight: 20, chrome: 14 },
  );
});

check(
  "a normal line height is 1.2 font sizes; unreadable values fall back",
  () => {
    assert.deepEqual(
      boxMetrics({
        lineHeight: "normal",
        fontSize: "15px",
        paddingTop: "0px",
        paddingBottom: "0px",
        borderTopWidth: "",
        borderBottomWidth: "",
      }),
      { lineHeight: 18, chrome: 0 },
    );
    assert.deepEqual(
      boxMetrics({
        lineHeight: "",
        fontSize: "",
        paddingTop: "",
        paddingBottom: "",
        borderTopWidth: "",
        borderBottomWidth: "",
      }),
      { lineHeight: FALLBACK_METRICS.lineHeight, chrome: 0 },
    );
  },
);

if (failures > 0) {
  console.error(
    `historyBox: ${failures} of ${checks} checks failed, ${checks - failures} passed`,
  );
  process.exit(1);
}
console.log(`historyBox: all ${checks} checks passed`);
