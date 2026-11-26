// DanyWhere: when a way of post-processing ends, its notice offers the next
// step instead of all the questions (sidecar qa 2026-11-26_1840, section 5,
// and 2026-11-26_2250, 4.5): set it up, top up, change the key's limit,
// manage the plan's usage, change the setup, sign in again. The reason comes
// from Rust as the notice's detail, so the action shows on notices from Rust.
import { describe, expect, test } from "bun:test";
import { act, screen } from "@testing-library/react";
import type { Notice } from "@/bindings";
import type { NavLocation } from "@/lib/navigation";
import { useNoticeStore } from "@/stores/noticeStore";
import { useNavigationRequest } from "@/stores/navigationRequestStore";
import { callsOf } from "@/test/bindings-mock";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";
import {
  BROWSER_PAGE,
  openedUrls,
  setupSettings,
  signInStarts,
} from "@/test/postProcessSetupFixtures";
import { NoticePanel } from "./NoticePanel";

const OPENROUTER = { providerId: "openrouter", provider: "OpenRouter" };
const CHATGPT = { providerId: "chatgpt", provider: "ChatGPT" };
const OPENAI = { providerId: "openai", provider: "OpenAI" };
const ANTHROPIC = { providerId: "anthropic", provider: "Anthropic" };
const GEMINI = { providerId: "gemini", provider: "Google Gemini" };

const failedWith = (reason: string, provider: object): Notice => ({
  code: "PostProcessFailed",
  level: "warning",
  source: "dictation",
  detail: JSON.stringify({ reason, ...provider }),
});

const showToast = async (shown: Notice) => {
  await renderPage(<NoticePanel />);
  await act(async () => {
    useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
  });
};

const button = (name: string) => screen.queryByRole("button", { name });
const POST_PROCESS_PAGE: NavLocation = {
  kind: "defaults",
  page: "postprocessing",
};

describe("a post-processing notice with its next step", () => {
  for (const reason of ["no-provider", "no-model", "no-api-key"]) {
    test(`${reason}: Set up opens the setup`, async () => {
      await showToast(failedWith(reason, OPENAI));
      await press(button("Set up")!);
      expect(useNavigationRequest.getState().requested).toEqual(
        POST_PROCESS_PAGE,
      );
      expect(rawTextIn()).toEqual([]);
    });
  }

  test("free-daily-limit: the sentence, and Top up opens OpenRouter's credit", async () => {
    await showToast(failedWith("free-daily-limit", OPENROUTER));
    expect(visibleText()).toContain(
      "The free dictations at OpenRouter are used up for today (50 a day); go on tomorrow, or top up once.",
    );
    await press(button("Top up")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/credits"]);
  });

  test("no-credit at OpenRouter: Top up", async () => {
    await showToast(failedWith("no-credit", OPENROUTER));
    await press(button("Top up")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/credits"]);
  });

  test("key-limit: Change limit opens the keys at OpenRouter", async () => {
    await showToast(failedWith("key-limit", OPENROUTER));
    expect(visibleText()).toContain(
      "The key's spending limit at OpenRouter is reached; change it there.",
    );
    await press(button("Change limit")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/keys"]);
  });

  // Only OpenRouter limits a key's spending; elsewhere limits are the
  // organization's or the project's, on the usage page (sidecar qa
  // 2026-11-26_2250, "Prüfung der Adressen").
  for (const [provider, url] of [
    [ANTHROPIC, "https://platform.claude.com/settings/spend-limits"],
    [GEMINI, "https://aistudio.google.com/usage"],
  ] as const) {
    test(`key-limit at ${provider.provider}: Manage usage opens its usage page`, async () => {
      await showToast(failedWith("key-limit", provider));
      expect(button("Change limit")).toBeNull();
      await press(button("Manage usage")!);
      expect(openedUrls()).toEqual([url]);
    });
  }

  test("plan-limit: Manage usage opens ChatGPT's usage", async () => {
    await showToast(failedWith("plan-limit", CHATGPT));
    expect(visibleText()).toContain(
      "Your plan's usage limit with ChatGPT is reached for now.",
    );
    await press(button("Manage usage")!);
    expect(openedUrls()).toEqual(["https://chatgpt.com/settings/usage"]);
  });

  test("plan-not-eligible: Change setup opens the questions", async () => {
    await showToast(failedWith("plan-not-eligible", CHATGPT));
    expect(visibleText()).toContain(
      "Your plan with ChatGPT doesn't include use in other apps.",
    );
    await press(button("Change setup")!);
    expect(useNavigationRequest.getState().requested).toEqual(
      POST_PROCESS_PAGE,
    );
  });

  test("sign-in-expired: Sign in again starts the sign-in", async () => {
    signInStarts();
    await showToast(failedWith("sign-in-expired", CHATGPT));
    expect(visibleText()).toContain(
      "The sign-in with ChatGPT has expired; sign in again.",
    );
    await press(button("Sign in again")!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE, null],
    ]);
    expect(useNavigationRequest.getState().requested).toEqual(
      POST_PROCESS_PAGE,
    );
  });

  // The answers say which way the new sign-in is for (sidecar qa
  // 2026-11-26_2250, D, H1).
  test("sign-in-expired at OpenRouter: Sign in again keeps to the way with credit", async () => {
    signInStarts();
    await renderPage(<NoticePanel />, {
      settings: setupSettings({
        texts_may_go: "not_kept",
        subscription: "none",
        next_step: "credit",
      }),
    });
    await act(async () => {
      const shown = failedWith("sign-in-expired", OPENROUTER);
      useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
    });
    await press(button("Sign in again")!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["openrouter", BROWSER_PAGE, "credit"],
    ]);
  });

  test("free-busy and rate-limited: a sentence, no action", async () => {
    await showToast(failedWith("free-busy", OPENROUTER));
    expect(visibleText()).toContain(
      "The free models at OpenRouter are busy right now; try again in a moment.",
    );
    await act(async () => {
      const shown = failedWith("rate-limited", {
        providerId: "groq",
        provider: "Groq",
      });
      useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
    });
    expect(visibleText()).toContain(
      "Groq limits how much may be sent per minute; wait a moment and dictate again.",
    );
    expect(button("Top up")).toBeNull();
    expect(button("Set up")).toBeNull();
  });

  test("a reason without a next step has no button", async () => {
    await showToast(failedWith("unreachable", OPENAI));
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "",
    ]);
  });

  test("in German", async () => {
    await switchTestLanguage("de");
    await showToast(failedWith("plan-limit", CHATGPT));
    expect(button("Nutzung verwalten")).not.toBeNull();
    await act(async () => {
      const shown = failedWith("no-api-key", OPENAI);
      useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
    });
    expect(button("Einrichten")).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });
});
