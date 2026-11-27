// DanyWhere: why a sign-in through the browser failed. The backend sends
// only a code (`SignInFailure`) with `post-process-sign-in-finished`; the
// card under the suggestion says what it means in a plain sentence, in the
// app's language, never the code itself.
import { beforeEach, describe, expect, test } from "bun:test";
import { act, screen } from "@testing-library/react";
import { PostProcessingSettings } from "../PostProcessingSettings";
import { DEFAULTS_SCOPE } from "@/lib/profileScope";
import { emitEvent } from "@/test/bindings-mock";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage, settle } from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import { rawTextIn, visibleText } from "@/test/visible";
import {
  answerStatus,
  notReady,
  setupSettings,
  signInStarts,
} from "@/test/postProcessSetupFixtures";
import type { SignInFailure, SignInProvider } from "@/bindings";

const FREE = /^Also to free services\./;
const NONE = /^None of these/;

const radio = (name: RegExp | string) => screen.getByRole("radio", { name });
const button = (name: string) => screen.queryByRole("button", { name });

/** The setup's answers that lead to `provider`'s sign-in, and its button. */
const WAYS: Record<SignInProvider, { subscription: RegExp; label: string }> = {
  chatgpt: { subscription: /^ChatGPT/, label: "Continue with ChatGPT" },
  openrouter: { subscription: NONE, label: "Connect OpenRouter" },
};

/** Starts `provider`'s sign-in and lets the backend end it with `error`. */
const signInFailsWith = async (
  provider: SignInProvider,
  error: SignInFailure,
) => {
  answerStatus(notReady());
  await renderPage(<PostProcessingSettings />, {
    settings: setupSettings(),
    scope: DEFAULTS_SCOPE,
  });
  await press(radio(FREE));
  await press(radio(WAYS[provider].subscription));
  signInStarts();
  await press(button(WAYS[provider].label)!);
  expect(visibleText()).toContain("Waiting for the browser …");
  await act(async () => {
    emitEvent("postProcessSignInFinished", {
      provider,
      ok: false,
      error,
      free_tier: null,
    });
  });
  await settle();
};

/** The card's line that says why the sign-in failed, if there is one. */
const failureLine = () =>
  document.querySelector("p.text-error")?.textContent ?? null;

type Expected = { provider: SignInProvider; en: string; de: string };

/** Each code a failed sign-in can end with, and what the user reads; a new
 * code in `SignInFailure` doesn't compile until it has its sentence here.
 * "cancelled" shows nothing: the sign-in just ends. */
const FAILURES = {
  timed_out: [
    {
      provider: "openrouter",
      en: "The sign-in waited 30 minutes and has expired.",
      de: "Die Anmeldung hat 30 Minuten gewartet und ist abgelaufen.",
    },
  ],
  denied: [
    {
      provider: "openrouter",
      en: "Access wasn't allowed.",
      de: "Der Zugriff wurde nicht erlaubt.",
    },
  ],
  not_eligible: [
    {
      provider: "chatgpt",
      en: "This ChatGPT account has no plan that DanyWhere can use (Go, Plus, Pro or Business).",
      de: "Dieses ChatGPT-Konto hat kein Abo, das DanyWhere nutzen kann (Go, Plus, Pro oder Business).",
    },
  ],
  no_free_model: [
    {
      provider: "openrouter",
      en: "OpenRouter has no free model that DanyWhere can use right now. Try again later, or top up once instead.",
      de: "OpenRouter hat gerade kein kostenloses Modell, das DanyWhere nutzen kann. Versuch es später noch einmal, oder lade stattdessen einmal auf.",
    },
  ],
  unreachable: [
    {
      provider: "openrouter",
      en: "Couldn't reach OpenRouter. Check the connection and try again.",
      de: "OpenRouter war nicht erreichbar. Prüf die Verbindung und versuch es noch einmal.",
    },
    {
      provider: "chatgpt",
      en: "Couldn't reach ChatGPT. Check the connection and try again.",
      de: "ChatGPT war nicht erreichbar. Prüf die Verbindung und versuch es noch einmal.",
    },
  ],
  refused: [
    {
      provider: "openrouter",
      en: "OpenRouter refused the sign-in. Try again.",
      de: "OpenRouter hat die Anmeldung abgelehnt. Versuch es noch einmal.",
    },
    {
      provider: "chatgpt",
      en: "ChatGPT refused the sign-in. Try again.",
      de: "ChatGPT hat die Anmeldung abgelehnt. Versuch es noch einmal.",
    },
  ],
  failed: [
    {
      provider: "openrouter",
      en: "The sign-in didn't work. Try again.",
      de: "Die Anmeldung hat nicht geklappt. Versuch es noch einmal.",
    },
  ],
} satisfies Record<Exclude<SignInFailure, "cancelled">, Expected[]>;

const CASES = (
  Object.entries(FAILURES) as [SignInFailure, Expected[]][]
).flatMap(([code, expected]) => expected.map((e) => ({ code, ...e })));

beforeEach(() => {
  setOsType("macos");
});

describe("post-processing setup: why a sign-in failed", () => {
  for (const { code, provider, en } of CASES) {
    test(`${provider}, ${code}: a plain sentence, never the code`, async () => {
      await signInFailsWith(provider, code);

      expect(failureLine()).toBe(en);
      expect(rawTextIn()).toEqual([]);
      expect(button("Try again")).not.toBeNull();
    });
  }

  for (const { code, provider, de } of CASES) {
    test(`${provider}, ${code}: in German`, async () => {
      await signInFailsWith(provider, code);
      await switchTestLanguage("de");
      await settle();

      expect(failureLine()).toBe(de);
      expect(rawTextIn()).toEqual([]);
      expect(button("Noch einmal versuchen")).not.toBeNull();
    });
  }

  test("cancelled: no failure, the button is back", async () => {
    await signInFailsWith("openrouter", "cancelled");

    expect(failureLine()).toBeNull();
    expect(rawTextIn()).toEqual([]);
    expect(button("Connect OpenRouter")).not.toBeNull();
    expect(visibleText()).not.toContain("Waiting for the browser");
  });
});
