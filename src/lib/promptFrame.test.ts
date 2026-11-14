import assert from "node:assert/strict";
import { hasOldParts } from "./promptFrame";

// The same cases as `output_and_the_transcript_tags_are_old_parts_the_word_alone_is_not`
// in src-tauri/src/prompt_frame.rs.
const old = [
  "Fix this:\n${output}",
  "<transcript>\n</transcript>\nFix it.",
  "The text is in the </transcript> tags.",
  "Use the <Transcript> tags.",
];
const fine = [
  "Clean up the transcript. Bereinige das Transkript.",
  "Fix the grammar.",
];

let failed = 0;
for (const prompt of old) {
  if (!hasOldParts(prompt)) {
    failed++;
    console.error(`not found: ${JSON.stringify(prompt)}`);
  }
}
for (const prompt of fine) {
  if (hasOldParts(prompt)) {
    failed++;
    console.error(`wrongly found: ${JSON.stringify(prompt)}`);
  }
}
assert.equal(failed, 0, `${failed} of ${old.length + fine.length} cases wrong`);

console.log("promptFrame: all assertions passed");
