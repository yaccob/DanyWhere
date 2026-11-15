// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/variant.test.ts
import assert from "node:assert";
import { buildVariant, RELEASE_PRODUCT_NAME } from "./variant";

const cases: Array<[string, () => void]> = [
  [
    "the release shows no mark",
    () => assert.strictEqual(buildVariant(RELEASE_PRODUCT_NAME), null),
  ],
  [
    "the release's product name is DanyWhere",
    () => assert.strictEqual(RELEASE_PRODUCT_NAME, "DanyWhere"),
  ],
  [
    "Dev is marked violet",
    () =>
      assert.deepStrictEqual(buildVariant("DanyWhere Dev"), {
        label: "Dev",
        background: "#7C3AED",
        foreground: "#FFFFFF",
      }),
  ],
  [
    "NoAX is marked orange, like its tray dot",
    () =>
      assert.deepStrictEqual(buildVariant("DanyWhere NoAX"), {
        label: "NoAX",
        background: "#F28C28",
        foreground: "#1A1A1A",
      }),
  ],
  [
    "any other test build is marked grey with its own name",
    () =>
      assert.deepStrictEqual(buildVariant("DanyWhere Test"), {
        label: "Test",
        background: "#6B7280",
        foreground: "#FFFFFF",
      }),
  ],
  [
    "a name that isn't DanyWhere's shows no mark",
    () => assert.strictEqual(buildVariant("Handy"), null),
  ],
  [
    "a trailing space alone shows no mark",
    () => assert.strictEqual(buildVariant("DanyWhere "), null),
  ],
];

let failed = 0;
for (const [name, check] of cases) {
  try {
    check();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(`variant: ${failed} of ${cases.length} checks failed`);
  process.exit(1);
}
console.log(`variant: all ${cases.length} checks passed`);
