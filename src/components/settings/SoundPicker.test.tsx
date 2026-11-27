// DanyWhere: the sound theme's preview plays the sounds of a dictation in
// the order a dictation plays them, each a step higher (user, 2026-11-27):
// start, instruction, stop, and the sound that ends it.
import { describe, expect, test } from "bun:test";
import { screen } from "@testing-library/react";
import { SoundPicker } from "./SoundPicker";
import { callsOf } from "@/test/bindings-mock";
import { settingsWith } from "@/test/fixtures";
import { press, renderPage } from "@/test/render";

const preview = () => screen.getByTitle(/^Preview sound theme/);

describe("Sound theme preview", () => {
  test("plays start, instruction, stop and done", async () => {
    await renderPage(<SoundPicker label="Sound Theme" description="" />);

    await press(preview());

    expect(callsOf("playTestSound")).toEqual([
      ["start"],
      ["instruction"],
      ["stop"],
      ["done"],
    ]);
  });

  test("ends with the waiting sound while the done sound is off", async () => {
    await renderPage(<SoundPicker label="Sound Theme" description="" />, {
      settings: settingsWith({ defaults: { done_sound: false } }),
    });

    await press(preview());

    expect(callsOf("playTestSound")).toEqual([
      ["start"],
      ["instruction"],
      ["stop"],
      ["clipboard"],
    ]);
  });
});
