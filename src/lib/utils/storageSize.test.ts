import assert from "node:assert/strict";
import {
  STORAGE_LIMITS,
  formatStorageSize,
  protectedExceedsLimit,
  storageLimitBytes,
} from "./storageSize";

// Every limit the backend knows, smallest first, in decimal units.
assert.deepEqual(STORAGE_LIMITS, [
  "mb_100",
  "mb_500",
  "gb_1",
  "gb_5",
  "unlimited",
]);
assert.equal(storageLimitBytes("mb_100"), 100_000_000);
assert.equal(storageLimitBytes("mb_500"), 500_000_000);
assert.equal(storageLimitBytes("gb_1"), 1_000_000_000);
assert.equal(storageLimitBytes("gb_5"), 5_000_000_000);
assert.equal(storageLimitBytes("unlimited"), null);

// Sizes as Finder shows them, with the locale's decimal separator. Some
// locales put a no-break space before the unit.
const size = (bytes: number, locale: string) =>
  formatStorageSize(bytes, locale).replace(/\s/g, " ");
assert.equal(size(100_000_000, "en"), "100 MB");
assert.equal(size(500_000_000, "de"), "500 MB");
assert.equal(size(1_000_000_000, "en"), "1 GB");
assert.equal(size(1_500_000_000, "de"), "1,5 GB");
assert.equal(size(2_345_678, "en"), "2.3 MB");
assert.equal(size(45_600_000, "en"), "46 MB");
assert.equal(size(0, "en"), "0 MB");

// Starred entries and transcribed files alone can exceed the limit.
const usage = { total_bytes: 700_000_000, protected_bytes: 600_000_000 };
assert.equal(protectedExceedsLimit(usage, "mb_500"), true);
assert.equal(protectedExceedsLimit(usage, "gb_1"), false);
assert.equal(protectedExceedsLimit(usage, "unlimited"), false);

console.log("storageSize: all assertions passed");
