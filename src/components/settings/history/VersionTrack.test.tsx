// DanyWhere: a version's chip names in its tooltip how the version was
// made; a post-processing with the instruction spoken for the dictation
// says so (sidecar qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md,
// F6).
import { describe, expect, test } from "bun:test";
import { render } from "@testing-library/react";
import React from "react";
import type { HistoryEntry, VersionAuthor, VersionStep } from "@/bindings";
import { VersionTrack } from "./VersionTrack";
import { switchTestLanguage } from "@/test/i18n";
import { rawTextIn } from "@/test/visible";

const step = (
  by: VersionAuthor,
  extra: Partial<VersionStep> = {},
): VersionStep => ({ by, at: null, prompt_name: null, model: null, ...extra });

/** A dictation and its post-processing, with or without the instruction. */
const entry = (
  spokenInstruction: boolean,
): Pick<HistoryEntry, "versions" | "timestamp"> => ({
  timestamp: 1764180000,
  versions: [
    { text: "dzien dobry", steps: [step("transcription")] },
    {
      text: "Dzień dobry.",
      steps: [
        step("ai", {
          prompt_name: "Dictation (DE/EN)",
          spoken_instruction: spokenInstruction,
        }),
      ],
    },
  ],
});

/** The track, open (the version below the top is shown). */
const track = (spokenInstruction: boolean) =>
  render(
    <VersionTrack
      entry={entry(spokenInstruction)}
      shown={0}
      matching={[]}
      onShow={() => {}}
      onMove={() => {}}
      onDelete={() => {}}
    />,
  );

/** The first line of each chip's tooltip: how its version was made. */
const madeBy = (): string[] =>
  Array.from(document.querySelectorAll("button[aria-pressed]")).map(
    (chip) => (chip.getAttribute("title") ?? "").split("\n")[0],
  );

describe("a version's chip", () => {
  test("names a post-processing with the spoken instruction", () => {
    track(true);
    expect(madeBy()[1]).toBe(
      "Post-processed with “Dictation (DE/EN)” + spoken instruction",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("names one without it as before", () => {
    track(false);
    expect(madeBy()[1]).toBe("Post-processed with “Dictation (DE/EN)”");
  });

  test("says it in German without raw keys", async () => {
    await switchTestLanguage("de");
    track(true);
    expect(madeBy()[1]).toBe(
      "Nachbearbeitet mit „Dictation (DE/EN)“ + gesprochene Anweisung",
    );
    expect(rawTextIn()).toEqual([]);
  });
});
