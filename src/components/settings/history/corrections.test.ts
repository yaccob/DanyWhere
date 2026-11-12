import assert from "node:assert/strict";
import {
  correctionOf,
  correctionsLostBy,
  isCorrected,
  isSaveShortcut,
  shownPart,
} from "./corrections";
import { shownText } from "./postProcessEntry";

let failures = 0;
const check = (name: string, run: () => void) => {
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

const raw = {
  transcription_text: "das Build hängt",
  post_processed_text: null,
  transcription_correction: null,
  post_processed_correction: null,
};
const rawCorrected = { ...raw, transcription_correction: "das Bild hängt" };
const processed = {
  ...raw,
  post_processed_text: "Das Build hängt.",
};
const processedCorrected = {
  ...processed,
  transcription_correction: "das Bild hängt",
  post_processed_correction: "Das Bild hängt.",
};

check("an entry without post-processing shows its transcription", () => {
  assert.equal(shownPart(raw, false), "transcription");
  assert.equal(shownPart(raw, true), "transcription");
});
check("a post-processed entry shows the post-processing", () => {
  assert.equal(shownPart(processed, false), "post_processing");
});
check("the original is the transcription", () => {
  assert.equal(shownPart(processed, true), "transcription");
});
check("a blank post-processing counts as none", () => {
  assert.equal(
    shownPart({ ...raw, post_processed_text: " " }, false),
    "transcription",
  );
});

check("a corrected transcription is shown instead of the original", () => {
  assert.equal(shownText(rawCorrected, false), "das Bild hängt");
  assert.equal(shownText(rawCorrected, true), "das Bild hängt");
});
check("a corrected post-processing is shown instead of its original", () => {
  assert.equal(shownText(processedCorrected, false), "Das Bild hängt.");
});
check("show original shows the corrected transcription", () => {
  assert.equal(shownText(processedCorrected, true), "das Bild hängt");
});
check("without corrections nothing changes", () => {
  assert.equal(shownText(processed, false), "Das Build hängt.");
  assert.equal(shownText(processed, true), "das Build hängt");
});
check("entries from before corrections still show", () => {
  assert.equal(
    shownText(
      { transcription_text: "old", post_processed_text: "Old." },
      false,
    ),
    "Old.",
  );
});

check("corrections are found per part", () => {
  assert.equal(
    correctionOf(processedCorrected, "transcription"),
    "das Bild hängt",
  );
  assert.equal(
    correctionOf(processedCorrected, "post_processing"),
    "Das Bild hängt.",
  );
  assert.equal(correctionOf(processed, "transcription"), null);
  assert.equal(isCorrected(rawCorrected, "transcription"), true);
  assert.equal(isCorrected(rawCorrected, "post_processing"), false);
});

check("post-processing again replaces only its own correction", () => {
  assert.equal(correctionsLostBy(processedCorrected, "post_process"), true);
  assert.equal(correctionsLostBy(rawCorrected, "post_process"), false);
  assert.equal(correctionsLostBy(processed, "post_process"), false);
});
check("transcribing again replaces every correction", () => {
  assert.equal(correctionsLostBy(rawCorrected, "transcribe"), true);
  assert.equal(
    correctionsLostBy(
      { ...processed, post_processed_correction: "x" },
      "transcribe_and_post_process",
    ),
    true,
  );
  assert.equal(correctionsLostBy(processed, "transcribe"), false);
});

check("Cmd+Enter and Ctrl+Enter save, Enter alone doesn't", () => {
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: true, ctrlKey: false }),
    true,
  );
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: false, ctrlKey: true }),
    true,
  );
  assert.equal(
    isSaveShortcut({ key: "Enter", metaKey: false, ctrlKey: false }),
    false,
  );
  assert.equal(
    isSaveShortcut({ key: "s", metaKey: true, ctrlKey: false }),
    false,
  );
});

if (failures > 0) {
  console.error(`corrections: ${failures} check(s) failed`);
  process.exit(1);
}
console.log("corrections: all assertions passed");
