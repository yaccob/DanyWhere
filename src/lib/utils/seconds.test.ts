import assert from "node:assert/strict";
import {
  formatMsAsSeconds,
  parseSecondsToMs,
  parseWholeSeconds,
} from "./seconds";

// Both decimal separators are accepted.
assert.equal(parseSecondsToMs("2,5", 0, 30), 2500);
assert.equal(parseSecondsToMs("2.5", 0, 30), 2500);
assert.equal(parseSecondsToMs(" 3 ", 0, 30), 3000);
assert.equal(parseSecondsToMs("0", 0, 30), 0);
// Rounded to tenths of a second.
assert.equal(parseSecondsToMs("1,26", 0, 30), 1300);
// Out of range or not a number.
assert.equal(parseSecondsToMs("31", 0, 30), null);
assert.equal(parseSecondsToMs("-1", 0, 30), null);
assert.equal(parseSecondsToMs("", 0, 30), null);
assert.equal(parseSecondsToMs("abc", 0, 30), null);
assert.equal(parseSecondsToMs("2,5,1", 0, 30), null);

// Display follows the locale's decimal separator, without trailing zeros.
assert.equal(formatMsAsSeconds(2500, "de"), "2,5");
assert.equal(formatMsAsSeconds(2500, "en"), "2.5");
assert.equal(formatMsAsSeconds(3000, "de"), "3");

// Whole seconds within the limits, e.g. for the post-processing time limits.
assert.equal(parseWholeSeconds("60", 5, 600), 60);
assert.equal(parseWholeSeconds(" 5 ", 5, 600), 5);
assert.equal(parseWholeSeconds("600", 5, 600), 600);
assert.equal(parseWholeSeconds("4", 5, 600), null);
assert.equal(parseWholeSeconds("601", 5, 600), null);
assert.equal(parseWholeSeconds("0", 5, 600), null);
assert.equal(parseWholeSeconds("2,5", 5, 600), null);
assert.equal(parseWholeSeconds("7.5", 5, 600), null);
assert.equal(parseWholeSeconds("-10", 5, 600), null);
assert.equal(parseWholeSeconds("", 5, 600), null);
assert.equal(parseWholeSeconds("abc", 5, 600), null);

console.log("seconds: all assertions passed");
