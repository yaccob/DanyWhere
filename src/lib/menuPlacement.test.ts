// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/menuPlacement.test.ts
//
// DanyWhere: a menu opened at a button stays inside the window. History's
// version menu opened at the start of its button and ran up to 110 px past
// the right edge of a 720 px window (qa 2026-11-22_0107, error 5).
import assert from "node:assert/strict";
import { MENU_MARGIN, menuOffset } from "./menuPlacement";

let total = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  total += 1;
  try {
    body();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message.split("\n")[0]}`);
  }
};

/** The menu's edges in the window for an anchor at `left`…`right`. */
const placed = (
  left: number,
  right: number,
  width: number,
  viewport: number,
  rtl: boolean,
) => {
  const offset = menuOffset({
    anchorLeft: left,
    anchorRight: right,
    menuWidth: width,
    viewportWidth: viewport,
    rtl,
  });
  return { left: left + offset, right: left + offset + width };
};

check("a menu that fits opens at the button's start", () => {
  assert.deepEqual(placed(100, 120, 211, 720, false), {
    left: 100,
    right: 311,
  });
});
check("near the right edge it moves left into the window (entry 805)", () => {
  const menu = placed(619, 639, 211, 720, false);
  assert.ok(menu.right <= 720 - MENU_MARGIN, `right edge ${menu.right}`);
  assert.ok(menu.left >= MENU_MARGIN, `left edge ${menu.left}`);
});
check("the same for entry 804", () => {
  const menu = placed(580, 600, 211, 720, false);
  assert.ok(menu.right <= 720 - MENU_MARGIN, `right edge ${menu.right}`);
});
check("right to left it opens at the button's right edge when it fits", () => {
  assert.deepEqual(placed(500, 520, 211, 720, true), { left: 309, right: 520 });
});
check("right to left near the left edge it moves right into the window", () => {
  const menu = placed(40, 60, 211, 720, true);
  assert.ok(menu.left >= MENU_MARGIN, `left edge ${menu.left}`);
  assert.ok(menu.right <= 720 - MENU_MARGIN, `right edge ${menu.right}`);
});
check("a menu wider than the window starts at its margin", () => {
  assert.equal(placed(300, 320, 900, 720, false).left, MENU_MARGIN);
});

// Dropdowns: the menu is at least as wide as its button and grows with its
// longest entry, so it often is wider than the button (qa 2026-11-22_0233:
// the chosen "Zwischenablage (Cmd+V)" broke into two lines in a menu as
// narrow as its button). Where it doesn't fit after the button's start, it
// opens towards the start instead, flush with the button's other edge, as
// the "+ Profile" menu did by hand.
check("a menu as wide as its button stays flush with it", () => {
  assert.deepEqual(placed(300, 500, 200, 720, false), {
    left: 300,
    right: 500,
  });
});
check("a wider menu at the right edge ends flush with its button", () => {
  // A 200 px button at a row's end, 24 px from the window's right edge.
  assert.deepEqual(placed(496, 696, 260, 720, false), {
    left: 436,
    right: 696,
  });
});
check(
  "right to left near the left edge it starts flush with the button",
  () => {
    assert.deepEqual(placed(24, 224, 260, 720, true), { left: 24, right: 284 });
  },
);
check("where neither edge fits, it moves just as far as it must", () => {
  // A narrow button in the middle, a menu wider than both sides allow.
  const menu = placed(300, 320, 600, 720, false);
  assert.deepEqual(menu, { left: 720 - MENU_MARGIN - 600, right: 712 });
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`menuPlacement: all ${total} checks passed`);
