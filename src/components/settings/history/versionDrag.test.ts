// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/history/versionDrag.test.ts
//
// DanyWhere: dragging a version's chip on History's track (H3): pressed,
// dragged once it moved far enough, dropped into a gap, or cancelled with
// Escape, which moves nothing and keeps the release's click from showing
// the chip (qa 2026-11-22_0107, error 4).
import assert from "node:assert/strict";
import {
  DRAG_THRESHOLD,
  cancelChipDrag,
  moveChip,
  pressChip,
  releaseChip,
} from "./versions";

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

/** Gaps every 10 pixels: x 0-9 is gap 0, 10-19 gap 1 … */
const slotAt = (x: number) => Math.max(0, Math.floor(x / 10));

check("a press that hardly moves isn't a drag", () => {
  const drag = moveChip(
    pressChip(2, 1, 25, 5),
    25 + DRAG_THRESHOLD - 1,
    5,
    slotAt,
  );
  assert.equal(drag.active, false);
  assert.deepEqual(releaseChip(drag), { move: null, swallowClick: false });
});
check("a drag drops the version into the gap under the pointer", () => {
  const drag = moveChip(pressChip(2, 1, 25, 5), 2, 5, slotAt);
  assert.equal(drag.active, true);
  assert.equal(drag.slot, 0);
  assert.deepEqual(releaseChip(drag), {
    move: { from: 2, to: 0 },
    swallowClick: true,
  });
});
check("dropped where it was, nothing moves", () => {
  const drag = moveChip(pressChip(2, 1, 25, 5), 31, 5, slotAt);
  assert.deepEqual(releaseChip(drag), { move: null, swallowClick: true });
});
check("Escape during a drag keeps the release's click from showing it", () => {
  const drag = moveChip(pressChip(2, 1, 25, 5), 2, 5, slotAt);
  assert.deepEqual(cancelChipDrag(drag), { swallowClick: true });
});
check("after Escape the release moves nothing", () =>
  assert.deepEqual(releaseChip(null), { move: null, swallowClick: false }),
);
check("Escape on a mere press leaves its click alone", () =>
  assert.deepEqual(cancelChipDrag(pressChip(2, 1, 25, 5)), {
    swallowClick: false,
  }),
);

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`versionDrag: all ${total} checks passed`);
