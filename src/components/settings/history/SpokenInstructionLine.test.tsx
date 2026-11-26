// DanyWhere: the instruction spoken for a dictation's post-processing,
// under the entry's text in History (sidecar
// qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md, F6 ii):
// "Spoken instruction: …" when the dictation had one, nothing when it had
// none, and its audio to play while it is kept.
import { describe, expect, test } from "bun:test";
import { render, screen } from "@testing-library/react";
import React from "react";
import { EMPTY_SEARCH, textMatcher } from "./historySearch";
import {
  SpokenInstructionLine,
  type SpokenInstructionLineProps,
} from "./SpokenInstructionLine";
import { switchTestLanguage } from "@/test/i18n";
import { press } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";

const AUDIO = "handy-1764180000-instruction.opus";

/** The line with `props`; the files it asked to play go to `loaded`. */
const line = (
  props: Partial<SpokenInstructionLineProps>,
  loaded: string[] = [],
) => (
  <SpokenInstructionLine
    instruction={null}
    audioFile={null}
    recordingRemoved={false}
    highlight={null}
    getAudioUrl={async (file) => {
      loaded.push(file);
      return null;
    }}
    {...props}
  />
);

const playButton = () => screen.queryByRole("button", { name: "Play" });

describe("the spoken instruction under an entry's text", () => {
  test("shows what was said", () => {
    render(line({ instruction: "translate it into Polish" }));
    expect(visibleText()).toBe("Spoken instruction: translate it into Polish");
    expect(rawTextIn()).toEqual([]);
    // No audio kept: nothing to play.
    expect(playButton()).toBeNull();
  });

  test("shows nothing for an entry without one", () => {
    const { container } = render(line({ audioFile: AUDIO }));
    expect(container.childElementCount).toBe(0);
    expect(container.textContent).toBe("");
  });

  test("says when nothing was heard", () => {
    render(line({ instruction: "  " }));
    expect(visibleText()).toBe("Spoken instruction: nothing heard");
  });

  test("plays its own audio", async () => {
    const loaded: string[] = [];
    render(line({ instruction: "make it formal", audioFile: AUDIO }, loaded));
    expect(
      screen.getByRole("group", { name: "Audio of the spoken instruction" }),
    ).toBeTruthy();
    const play = playButton();
    expect(play).not.toBeNull();
    await press(play as HTMLElement);
    expect(loaded).toEqual([AUDIO]);
  });

  test("has no audio once the storage limit removed the recordings", () => {
    render(
      line({
        instruction: "make it formal",
        audioFile: AUDIO,
        recordingRemoved: true,
      }),
    );
    expect(visibleText()).toBe("Spoken instruction: make it formal");
    expect(playButton()).toBeNull();
  });

  test("marks where the search matches", () => {
    const { container } = render(
      line({
        instruction: "translate it into Polish",
        highlight: textMatcher({ ...EMPTY_SEARCH, text: "polish" }),
      }),
    );
    const marks = Array.from(container.querySelectorAll("mark"));
    expect(marks.map((mark) => mark.textContent)).toEqual(["Polish"]);
  });

  test("speaks German without raw keys", async () => {
    await switchTestLanguage("de");
    const { unmount } = render(
      line({ instruction: "übersetze ins Polnische", audioFile: AUDIO }),
    );
    expect(visibleText()).toContain(
      "Gesprochene Anweisung: übersetze ins Polnische",
    );
    expect(
      screen.getByRole("group", { name: "Ton der gesprochenen Anweisung" }),
    ).toBeTruthy();
    expect(rawTextIn()).toEqual([]);
    unmount();

    render(line({ instruction: "" }));
    expect(visibleText()).toBe("Gesprochene Anweisung: nichts verstanden");
  });
});
