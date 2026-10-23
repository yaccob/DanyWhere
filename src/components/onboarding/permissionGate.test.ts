import assert from "node:assert/strict";
import { permissionsSatisfied } from "./permissionGate";

const all = { accessibility: true, microphone: true };
const noAccessibility = { accessibility: false, microphone: true };
const noMicrophone = { accessibility: true, microphone: false };

// macOS normally needs both permissions.
assert.equal(permissionsSatisfied("macos", all, false), true);
assert.equal(permissionsSatisfied("macos", noAccessibility, false), false);
assert.equal(permissionsSatisfied("macos", noMicrophone, false), false);

// Skipping Accessibility lets the user through with the microphone alone...
assert.equal(permissionsSatisfied("macos", noAccessibility, true), true);
// ...but never without the microphone.
assert.equal(
  permissionsSatisfied(
    "macos",
    { accessibility: false, microphone: false },
    true,
  ),
  false,
);

// Other platforms are unaffected by the skip flag.
assert.equal(permissionsSatisfied("windows", noAccessibility, false), true);
assert.equal(permissionsSatisfied("windows", noMicrophone, true), false);
assert.equal(permissionsSatisfied("other", noMicrophone, false), true);

console.log("permissionGate: all assertions passed");
