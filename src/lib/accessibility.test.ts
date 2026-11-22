// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/accessibility.test.ts
import assert from "node:assert/strict";
import { choseToGoWithout, runsWithoutAccessibility } from "./accessibility";

// The stored choice: only an explicit `true` counts, as before (an older
// store has no field, a failed read no settings).
assert.equal(choseToGoWithout({ accessibility_skipped: true }), true);
assert.equal(choseToGoWithout({ accessibility_skipped: false }), false);
assert.equal(choseToGoWithout({}), false);
assert.equal(choseToGoWithout(null), false);
assert.equal(choseToGoWithout(undefined), false);

// The mode is macOS's alone: chosen elsewhere, it changes nothing.
for (const os of ["macos", "windows", "linux", "unknown"]) {
  for (const skipped of [true, false, undefined]) {
    assert.equal(
      runsWithoutAccessibility(os, { accessibility_skipped: skipped }),
      os === "macos" && skipped === true,
      `${os}, skipped ${skipped}`,
    );
  }
}
assert.equal(runsWithoutAccessibility("macos", null), false);

console.log("accessibility: all checks passed");
