// DanyWhere: History's ✨ for an entry with a spoken instruction (sidecar
// qa/2026-11-26_1720_nachbearbeitung-gesprochene-anweisung.md, F6): a click
// on the wand post-processes with the instruction, as the dictation did;
// its menu offers the same without it.
import { describe, expect, test } from "bun:test";
import { render, screen } from "@testing-library/react";
import React from "react";
import { PostProcessButton } from "./PostProcessButton";
import type { PromptMenuItem } from "./promptMenu";
import { switchTestLanguage } from "@/test/i18n";
import { press } from "@/test/render";
import { isVisible, rawTextIn } from "@/test/visible";

const PROMPTS: PromptMenuItem[] = [
  {
    id: "prompt_1795300000001",
    label: "Dictation",
    tooltip: "Dictation",
    active: true,
  },
];

/** What the button's clicks ran, in order. */
type Ran = string[];

const button = (
  ran: Ran,
  props: Partial<React.ComponentProps<typeof PostProcessButton>> = {},
) => (
  <PostProcessButton
    availability="ready"
    onRun={() => ran.push("with")}
    prompts={PROMPTS}
    onRunPrompt={(id) => ran.push(id)}
    menuUsable
    {...props}
  />
);

const withoutInstruction = (ran: Ran) => () => ran.push("without");

const openMenu = () =>
  press(screen.getByRole("button", { name: "Post-process with one prompt" }));

const withoutItem = () =>
  screen.queryByRole("menuitem", { name: "Without the spoken instruction" });

describe("✨ for an entry with a spoken instruction", () => {
  test("its menu offers to post-process without the instruction", async () => {
    const ran: Ran = [];
    render(button(ran, { onRunWithoutInstruction: withoutInstruction(ran) }));
    await openMenu();
    const item = withoutItem();
    expect(item).not.toBeNull();
    expect(isVisible(item!)).toBe(true);
    expect(rawTextIn()).toEqual([]);
    await press(item!);
    expect(ran).toEqual(["without"]);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  test("a click on the wand still runs with the instruction", async () => {
    const ran: Ran = [];
    const { container } = render(
      button(ran, { onRunWithoutInstruction: withoutInstruction(ran) }),
    );
    await press(container.querySelector("button")!);
    expect(ran).toEqual(["with"]);
  });

  test("an entry without one gets no such item", async () => {
    const ran: Ran = [];
    render(button(ran));
    await openMenu();
    expect(withoutItem()).toBeNull();
    expect(screen.getAllByRole("menuitemcheckbox")).toHaveLength(1);
  });

  test("the item is there also without a prompt to choose", async () => {
    const ran: Ran = [];
    render(
      button(ran, {
        prompts: [],
        onRunWithoutInstruction: withoutInstruction(ran),
      }),
    );
    await openMenu();
    expect(withoutItem()).not.toBeNull();
  });

  test("not while post-processing can't run", async () => {
    const ran: Ran = [];
    render(
      button(ran, {
        availability: "off",
        menuUsable: false,
        onRunWithoutInstruction: withoutInstruction(ran),
      }),
    );
    await openMenu();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(ran).toEqual([]);
  });

  test("in German", async () => {
    await switchTestLanguage("de");
    const ran: Ran = [];
    render(button(ran, { onRunWithoutInstruction: withoutInstruction(ran) }));
    await press(
      screen.getByRole("button", { name: "Mit einem Prompt nachbearbeiten" }),
    );
    expect(
      screen.queryByRole("menuitem", {
        name: "Ohne die gesprochene Anweisung",
      }),
    ).not.toBeNull();
  });
});
