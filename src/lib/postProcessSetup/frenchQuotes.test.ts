// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/postProcessSetup/frenchQuotes.test.ts
//
// DanyWhere: French puts a non-breaking space inside « … », so a line of
// the post-processing setup never breaks right after « or before » (sidecar
// qa 2026-11-26_2250, strand ZF: “« Rouvrir la page »” broke after «).
// tools/i18n/post_process_setup_strings.py writes them.
import assert from "node:assert/strict";
import fr from "../../i18n/locales/fr/translation.json";

const NBSP = " ";
const SECTIONS = [
  "settings.postProcessing.setup",
  "onboarding.postProcessSetup",
  "errors.postProcessFailure",
];

const at = (dotted: string): unknown =>
  dotted
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      fr,
    );

/** Every string under `node`, with its key. */
const strings = (node: unknown, path: string): [string, string][] => {
  if (typeof node === "string") return [[path, node]];
  if (!node || typeof node !== "object") return [];
  return Object.entries(node).flatMap(([key, value]) =>
    strings(value, `${path}.${key}`),
  );
};

let total = 0;
const failures: string[] = [];
for (const section of SECTIONS) {
  for (const [key, text] of strings(at(section), section)) {
    if (!text.includes("«") && !text.includes("»")) continue;
    total += 1;
    try {
      assert.ok(!/« |«$/.test(text), "a plain space after «");
      assert.ok(!/ »/.test(text), "a plain space before »");
      assert.ok(
        !/«[^ ]/.test(text),
        `« without ${JSON.stringify(NBSP)} after it`,
      );
      assert.ok(
        !/[^ ]»/.test(text),
        `» without ${JSON.stringify(NBSP)} before it`,
      );
    } catch (error) {
      failures.push(`${key}: ${(error as Error).message.split("\n")[0]}`);
    }
  }
}

assert.ok(total > 0, "no French string with « … » found");
if (failures.length > 0) {
  console.error(failures.join("\n"));
  console.error(`frenchQuotes: ${failures.length} of ${total} checks failed`);
  process.exit(1);
}
console.log(`frenchQuotes: all ${total} checks passed`);
