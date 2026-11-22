// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/escapeLayers.test.ts
//
// DanyWhere: Escape closes what is open in front, heard on the window: WebKit
// gives a clicked button no keyboard focus, so a key handler on a dropdown or
// a dragged chip never hears it (qa 2026-11-22_0107, error 4; qa
// 2026-11-22_0233, "Escape schließt die Auswahllisten nicht"). Only the layer
// in front closes: a list open in a dialog closes first, the dialog on the
// next press.
import assert from "node:assert/strict";
import { cancelOnEscape, escapeLayerOpen } from "./escapeLayers";

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

/** A key press as the window gets it; `stopped` says whether it was kept
 * from travelling on to the document (where a dialog listens). The flag
 * itself is cleared when the dispatch ends, so the call is recorded. */
const press = (
  target: EventTarget,
  key: string,
  extra: Partial<Pick<KeyboardEvent, "isComposing" | "keyCode">> = {},
): Event & { stopped: boolean } => {
  const event = Object.assign(
    new Event("keydown", { cancelable: true, bubbles: true }),
    { key, isComposing: false, keyCode: 0, stopped: false, ...extra },
  );
  const stop = event.stopPropagation.bind(event);
  event.stopPropagation = () => {
    event.stopped = true;
    stop();
  };
  target.dispatchEvent(event);
  return event;
};

// --- One layer: what the drags had (moved from dragCancel.test.ts) ---------
check("Escape on the window closes the layer", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  const event = press(target, "Escape");
  assert.equal(cancelled, 1);
  assert.equal(event.defaultPrevented, true);
});
check("other keys leave the layer alone", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  press(target, "Enter");
  press(target, "ArrowDown");
  assert.equal(cancelled, 0);
});
check("once the layer is gone, Escape is the page's again", () => {
  const target = new EventTarget();
  let cancelled = 0;
  const stop = cancelOnEscape(target, () => (cancelled += 1));
  stop();
  const event = press(target, "Escape");
  assert.equal(cancelled, 0);
  assert.equal(event.defaultPrevented, false);
});
check(
  "the press stops at the window, so a dialog behind never hears it",
  () => {
    const target = new EventTarget();
    cancelOnEscape(target, () => {});
    const event = press(target, "Escape");
    assert.equal(event.stopped, true);
  },
);

// --- Several layers: only the one in front closes --------------------------
check("a list open in front of a menu: Escape closes only the list", () => {
  const target = new EventTarget();
  const closed: string[] = [];
  cancelOnEscape(target, () => closed.push("menu"));
  cancelOnEscape(target, () => closed.push("list"));
  press(target, "Escape");
  assert.deepEqual(closed, ["list"]);
});
check("the next Escape closes the menu behind", () => {
  const target = new EventTarget();
  const closed: string[] = [];
  cancelOnEscape(target, () => closed.push("menu"));
  cancelOnEscape(target, () => closed.push("list"));
  press(target, "Escape");
  press(target, "Escape");
  assert.deepEqual(closed, ["list", "menu"]);
});
check("a closed layer is gone before its owner stops listening", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  press(target, "Escape");
  const second = press(target, "Escape");
  assert.equal(cancelled, 1);
  assert.equal(second.defaultPrevented, false);
});
check("stopping a layer behind leaves the one in front in charge", () => {
  const target = new EventTarget();
  const closed: string[] = [];
  const stopMenu = cancelOnEscape(target, () => closed.push("menu"));
  cancelOnEscape(target, () => closed.push("list"));
  stopMenu();
  press(target, "Escape");
  assert.deepEqual(closed, ["list"]);
});

// --- The rest of the window ------------------------------------------------
check("a window listener added later doesn't hear the press", () => {
  const target = new EventTarget();
  cancelOnEscape(target, () => {});
  let heard = 0;
  target.addEventListener("keydown", () => (heard += 1), true);
  press(target, "Escape");
  assert.equal(heard, 0);
});
check("escapeLayerOpen tells an earlier page listener to leave Escape", () => {
  const target = new EventTarget();
  assert.equal(escapeLayerOpen(target), false);
  const stop = cancelOnEscape(target, () => {});
  assert.equal(escapeLayerOpen(target), true);
  stop();
  assert.equal(escapeLayerOpen(target), false);
});
check("Escape while an input method composes is the input's", () => {
  const target = new EventTarget();
  let cancelled = 0;
  cancelOnEscape(target, () => (cancelled += 1));
  press(target, "Escape", { isComposing: true });
  press(target, "Escape", { keyCode: 229 });
  assert.equal(cancelled, 0);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`escapeLayers: all ${total} checks passed`);
