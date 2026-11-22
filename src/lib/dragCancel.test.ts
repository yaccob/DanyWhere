// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/dragCancel.test.ts
//
// DanyWhere: Escape ends a pointer drag without dropping, heard on the
// window: WebKit gives a clicked button no keyboard focus, so a key handler
// on the dragged chip or handle never hears it (qa 2026-11-22_0107, error 4
// and the hint in C2).
import assert from "node:assert/strict";
import { cancelOnEscape } from "./dragCancel";

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

/** A key press as the window gets it. */
const press = (target: EventTarget, key: string): Event => {
  const event = Object.assign(new Event("keydown", { cancelable: true }), {
    key,
  });
  target.dispatchEvent(event);
  return event;
};

check("Escape on the window ends the drag", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  const event = press(target, "Escape");
  assert.equal(cancelled, 1);
  assert.equal(event.defaultPrevented, true);
});
check("other keys leave the drag alone", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  press(target, "Enter");
  press(target, "ArrowDown");
  assert.equal(cancelled, 0);
});
check("once the drag is over, Escape is the page's again", () => {
  const target = new EventTarget();
  let cancelled = 0;
  const stop = cancelOnEscape(target, () => (cancelled += 1));
  stop();
  const event = press(target, "Escape");
  assert.equal(cancelled, 0);
  assert.equal(event.defaultPrevented, false);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`dragCancel: all ${total} checks passed`);
