// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/dragCancel.test.ts
//
// DanyWhere: a pointer drag follows the pointer on the window. Escape during
// a drag is tested with the other layers in escapeLayers.test.ts.
import assert from "node:assert/strict";
import { followPointer } from "./dragCancel";

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

// The prompt list shows the new order while a prompt is dragged. Dragged
// down, React moves the dragged row's own node, which drops the handle's
// pointer capture, so its pointerup never came and the drag hung until
// Escape (qa 2026-11-22_0233, point 10). The drag follows the pointer on the
// window instead, where every move and release arrives.
const pointer = (target: EventTarget, type: string, clientY = 0) =>
  target.dispatchEvent(Object.assign(new Event(type), { clientY }));
const recorder = () => {
  const seen: string[] = [];
  return {
    seen,
    handlers: {
      onMove: (y: number) => seen.push(`move ${y}`),
      onEnd: () => seen.push("end"),
      onCancel: () => seen.push("cancel"),
    },
  };
};

check("a release on the window ends the drag", () => {
  const target = new EventTarget();
  const { seen, handlers } = recorder();
  followPointer(target, handlers);
  pointer(target, "pointerup");
  assert.deepEqual(seen, ["end"]);
});
check("moves on the window follow the pointer", () => {
  const target = new EventTarget();
  const { seen, handlers } = recorder();
  followPointer(target, handlers);
  pointer(target, "pointermove", 120);
  pointer(target, "pointermove", 180);
  assert.deepEqual(seen, ["move 120", "move 180"]);
});
check("a cancelled pointer cancels the drag", () => {
  const target = new EventTarget();
  const { seen, handlers } = recorder();
  followPointer(target, handlers);
  pointer(target, "pointercancel");
  assert.deepEqual(seen, ["cancel"]);
});
check("once the drag is over, the pointer is the page's again", () => {
  const target = new EventTarget();
  const { seen, handlers } = recorder();
  const stop = followPointer(target, handlers);
  stop();
  pointer(target, "pointermove", 50);
  pointer(target, "pointerup");
  assert.deepEqual(seen, []);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`dragCancel: all ${total} checks passed`);
