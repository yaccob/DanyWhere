// DanyWhere: the post-processing setup at the top of the page "Post Process"
// (sidecar qa 2026-11-26_1840, sections 4 to 6, and 2026-11-26_2250, 4.5).
// Questions anyone can answer come one after the other, the suggestion
// under them changes with the answers, and its button sets the way up: a
// sign-in through the browser (OpenRouter, ChatGPT), a guided key, or a
// server of one's own. Once set up, a card says what it is connected to and
// tries a sample sentence; today's provider settings wait under "Advanced"
// or "All providers …".
import { beforeEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { PostProcessingSettings } from "../PostProcessingSettings";
import { DEFAULTS_SCOPE } from "@/lib/profileScope";
import { OpenManualProvider } from "@/hooks/useOpenManual";
import { useNoticeStore } from "@/stores/noticeStore";
import { callsOf, emitEvent, err, mockCommand, ok } from "@/test/bindings-mock";
import { switchTestLanguage } from "@/test/i18n";
import { press, raisedNotices, renderPage, settle } from "@/test/render";
import { setOsType } from "@/test/tauri-mock";
import { isVisible, rawTextIn, visibleText } from "@/test/visible";
import {
  BROWSER_PAGE,
  BROWSER_PAGE_DE,
  GiB,
  answerStatus,
  failed,
  lastAnswers,
  notReady,
  openedUrls,
  ready,
  setupSettings,
  signInStarts,
} from "@/test/postProcessSetupFixtures";
import type { SetupStatus } from "@/bindings";
import type { SetupAnswers } from "@/lib/postProcessSetup/questions";
import { runFailureAction } from "@/lib/postProcessSetup/actions";

const WORK = "profile_1795300000001";

const TITLE = "Improve Your Texts";
const Q1 = "1. Where may your texts go?";
const Q2 = "2. Do you already have one of these subscriptions?";
const Q3 = "3. How should it go on?";
const FREE = /^Also to free services\./;
const NOT_KEPT =
  /^Only to a service that processes them and doesn't keep them\./;
const NOWHERE = /^Nowhere\./;
const NONE = /^None of these/;

const CONNECT_OPENROUTER = "Connect OpenRouter";
const CONTINUE_CHATGPT = "Continue with ChatGPT";
const SAMPLE_TEXT =
  "um so I'll send you that tomorrow comma okay question mark";

const radio = (name: RegExp | string) => screen.getByRole("radio", { name });
const button = (name: string | RegExp) =>
  screen.queryByRole("button", { name });

/** The page of the defaults with the setup's answers as kept and the
 * backend's status. */
const renderSetup = async (
  status: SetupStatus | (() => SetupStatus) = notReady(),
  answers: SetupAnswers | null = null,
  kept: { finished?: boolean; skipped?: boolean } = {},
) => {
  answerStatus(status);
  return renderPage(<PostProcessingSettings />, {
    settings: setupSettings(answers, {}, kept),
    scope: DEFAULTS_SCOPE,
  });
};

const typeInto = async (field: HTMLElement, value: string) => {
  await act(async () => {
    fireEvent.change(field, { target: { value } });
  });
  await settle();
};

beforeEach(() => {
  setOsType("macos");
});

describe("post-processing setup: the questions", () => {
  test("the quick way: two answers and one button", async () => {
    await renderSetup();

    const text = visibleText();
    expect(text).toContain(TITLE);
    expect(text).toContain(Q1);
    expect(text).not.toContain(Q2);

    await press(radio(FREE));
    expect(visibleText()).toContain(Q2);
    // Alphabetically, each provider with the plans that count, "none" last.
    const options = screen
      .getAllByRole("radio")
      .map((option) => option.textContent ?? "");
    const subscriptions = options.slice(3);
    expect(subscriptions).toEqual([
      "ChatGPT Go, Plus, Pro, or Business",
      "Claude Max, Ultra, or Team",
      "Google AI Pro or Ultra",
      "Hugging Face PRO",
      "Ollama Pro or Max",
      "None of these",
    ]);

    await press(radio(NONE));
    const after = visibleText();
    expect(after).not.toContain(Q3);
    expect(after).toContain("Suggestion: OpenRouter, free");
    expect(after).toContain(
      "An account with OpenRouter, one click on “Authorize”, done. Up to 50 dictations a day.",
    );
    // What the trial found: Google or GitHub, and the welcome's payment step.
    expect(after).toContain("Sign up with Google or GitHub");
    expect(after).toContain("“I'll do this later”");
    expect(lastAnswers()).toEqual({
      answers: {
        texts_may_go: "free_services",
        subscription: "none",
        next_step: null,
      },
      finished: false,
      skipped: false,
    });

    signInStarts();
    await press(button(CONNECT_OPENROUTER)!);
    // With the texts the browser shows after the callback.
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["openrouter", BROWSER_PAGE, "free"],
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("texts that may not be kept, without a subscription: three questions", async () => {
    await renderSetup();
    await press(radio(NOT_KEPT));
    await press(radio(NONE));

    expect(visibleText()).toContain(Q3);
    const steps = screen
      .getAllByRole("radio")
      .slice(9)
      .map((option) => option.textContent ?? "");
    expect(steps).toEqual([
      "Top up once from $5, without a subscription",
      "Free with Groq, with a key to copy",
      "On this computer (slower, from 16 GB of memory)",
    ]);

    await press(radio(/^Top up once/));
    const text = visibleText();
    expect(text).toContain("Suggestion: OpenRouter with credit");
    expect(text).toContain(
      "DanyWhere picks a model that doesn't use your texts for training.",
    );
    expect(button(CONNECT_OPENROUTER)).not.toBeNull();
    expect(lastAnswers()?.answers).toEqual({
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "credit",
    });
    expect(rawTextIn()).toEqual([]);
  });

  // The answer "doesn't keep them" holds before the top-up too (sidecar qa
  // 2026-11-26_2250, D, H1): the sign-in says it is for credit, the free
  // models never come into it, and nothing goes out until there is credit.
  test("the way with credit: no free models before the top-up, and the sign-in says so", async () => {
    await renderSetup();
    await press(radio(NOT_KEPT));
    await press(radio(NONE));
    await press(radio(/^Top up once/));

    const text = visibleText();
    expect(text).not.toContain("“I'll do this later”");
    expect(text).not.toContain("free models");
    expect(text).toContain(
      "Until the account has credit, DanyWhere sends nothing.",
    );
    signInStarts();
    await press(button(CONNECT_OPENROUTER)!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["openrouter", BROWSER_PAGE, "credit"],
    ]);
    expect(rawTextIn()).toEqual([]);
  });

  test("8 GB: this computer is greyed out, with the reason", async () => {
    await renderSetup(notReady({ memory_bytes: 8 * GiB }));
    await press(radio(NOT_KEPT));
    await press(radio(NONE));

    const local = radio(/^On this computer/);
    expect(local.textContent).toBe(
      "On this computer: needs 16 GB, this Mac has 8 GB.",
    );
    expect(local.getAttribute("aria-disabled")).toBe("true");
    await press(local);
    expect(lastAnswers()?.answers?.next_step ?? null).toBeNull();
  });

  test("Groq: a guided key", async () => {
    await renderSetup();
    await press(radio(NOT_KEPT));
    await press(radio(NONE));
    await press(radio(/^Free with Groq/));

    const text = visibleText();
    expect(text).toContain("Suggestion: a key from Groq");
    expect(text).toContain("Continue with Google");
    // The trial: creating the key asks for a check that a person clicks.
    expect(text).toContain("confirm with a click that you're a person");
    await press(button("Open GroqCloud")!);
    expect(openedUrls()).toEqual(["https://console.groq.com/keys"]);
  });

  test("nowhere, on 8 GB: not possible here, a server of one's own", async () => {
    answerStatus(notReady({ memory_bytes: 8 * GiB }));
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null),
      scope: DEFAULTS_SCOPE,
      acceptWrites: true,
    });
    await press(radio(NOWHERE));

    const text = visibleText();
    expect(text).not.toContain(Q2);
    expect(text).toContain("Not possible on this computer");
    expect(text).toContain("this Mac has 8 GB");
    await press(button("Set up a server of your own")!);
    // The custom provider's form opens with it chosen, as under All
    // providers …: nothing is set up before an address is entered and
    // tried (sidecar qa 2026-11-26_2250, D, N9).
    expect(callsOf("applyPostProcessChoice")).toEqual([]);
    expect(callsOf("setProfileValue")).toEqual([
      [DEFAULTS_SCOPE, "post_process_provider_id", "custom"],
    ]);
    // Its address and model are among today's settings.
    expect(visibleText()).toContain("Base URL");
  });

  test("nowhere, on 32 GB: the local model comes with a later version", async () => {
    await renderSetup(notReady({ memory_bytes: 32 * GiB }));
    await press(radio(NOWHERE));

    const text = visibleText();
    expect(text).toContain("On this computer: comes with a later version");
    expect(button("Set up a server of your own")).not.toBeNull();
  });

  test("changing the first answer drops the ones that no longer apply", async () => {
    await renderSetup(notReady(), {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "credit",
    });
    expect(visibleText()).toContain(Q3);
    await press(radio(FREE));
    expect(visibleText()).not.toContain(Q3);
    expect(lastAnswers()?.answers).toEqual({
      texts_may_go: "free_services",
      subscription: "none",
      next_step: null,
    });
  });

  test("Later puts it off, and Set up brings the questions back", async () => {
    await renderSetup();
    await press(button("Later")!);
    expect(lastAnswers()).toEqual({
      answers: { texts_may_go: null, subscription: null, next_step: null },
      finished: false,
      skipped: true,
    });
    expect(visibleText()).toContain("Post-processing isn't set up yet.");
    expect(visibleText()).not.toContain(Q1);
    await press(button("Set up")!);
    expect(visibleText()).toContain(Q1);
  });

  test("All providers … shows today's settings", async () => {
    await renderSetup();
    expect(visibleText()).toContain("I know my way around:");
    expect(visibleText()).not.toContain("API Key");
    await press(button("All providers …")!);
    expect(visibleText()).toContain("API Key");
    expect(visibleText()).toContain("Provider");
  });

  test("a profile's page has no setup: it shows the profile's provider", async () => {
    answerStatus(notReady());
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null, {
        profiles: [{ id: WORK, name: "Work" }],
      }),
      scope: { kind: "profile", id: WORK },
    });
    expect(visibleText()).not.toContain(TITLE);
    expect(visibleText()).toContain("API Key");
  });
});

describe("post-processing setup: signing in through the browser", () => {
  const toOpenRouter = async () => {
    await renderSetup();
    await press(radio(FREE));
    await press(radio(NONE));
    signInStarts();
    await press(button(CONNECT_OPENROUTER)!);
  };

  test("waits for the browser, with Open the page again and Cancel", async () => {
    await toOpenRouter();

    const text = visibleText();
    expect(text).toContain("Waiting for the browser …");
    expect(text).toContain("Open the page again");
    // The tips stay while it waits: a new user signs up first.
    expect(text).toContain("Sign up with Google or GitHub");
    await press(button("Open the page again")!);
    expect(callsOf("reopenPostProcessSignIn")).toEqual([[]]);

    await press(button("Cancel")!);
    expect(callsOf("cancelPostProcessSignIn")).toEqual([[]]);
    expect(visibleText()).not.toContain("Waiting for the browser …");
    expect(button(CONNECT_OPENROUTER)).not.toBeNull();
  });

  test("a finished sign-in shows the card and tries the sample", async () => {
    let status = notReady();
    answerStatus(() => status);
    mockCommand("runPostProcessSample", async () =>
      ok({
        output: "So I'll send you that tomorrow, okay?",
        model: "nvidia/nemotron-3.5-super-vl:free",
        millis: 7300,
      }),
    );
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null),
      scope: DEFAULTS_SCOPE,
    });
    await press(radio(FREE));
    await press(radio(NONE));
    signInStarts();
    await press(button(CONNECT_OPENROUTER)!);

    status = ready();
    await act(async () => {
      emitEvent("postProcessSignInFinished", {
        provider: "openrouter",
        ok: true,
        error: null,
        free_tier: true,
      });
    });
    await settle();

    const text = visibleText();
    expect(text).toContain("Connected to OpenRouter");
    expect(text).toContain("Free models, up to 50 dictations a day");
    expect(text).not.toContain("Waiting for the browser …");
    expect(callsOf("runPostProcessSample")).toEqual([[SAMPLE_TEXT]]);
    expect(text).toContain(`Spoken: “${SAMPLE_TEXT}”`);
    expect(text).toContain("Result: “So I'll send you that tomorrow, okay?”");
    expect(lastAnswers()?.finished).toBe(true);
    expect(rawTextIn()).toEqual([]);
  });

  test("a sign-in that waited too long says so and can start again", async () => {
    await toOpenRouter();
    await act(async () => {
      emitEvent("postProcessSignInFinished", {
        provider: "openrouter",
        ok: false,
        error: "timed_out",
        free_tier: null,
      });
    });
    await settle();
    expect(visibleText()).toContain(
      "The sign-in waited 30 minutes and has expired.",
    );
    await press(button("Try again")!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["openrouter", BROWSER_PAGE, "free"],
      ["openrouter", BROWSER_PAGE, "free"],
    ]);
  });

  test("ChatGPT: OpenAI's button, the notice before the first use", async () => {
    await renderSetup();
    await press(radio(FREE));
    await press(radio(/^ChatGPT/));

    const text = visibleText();
    expect(text).toContain("Suggestion: your ChatGPT plan");
    expect(text).toContain(
      "Eligible usage in this app uses your ChatGPT plan. Manage usage in your ChatGPT settings.",
    );
    expect(text).not.toContain("OpenAI doesn't describe");
    signInStarts();
    await press(button(CONTINUE_CHATGPT)!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE, null],
    ]);
  });

  test("ChatGPT for texts that may not be kept says what OpenAI doesn't describe", async () => {
    await renderSetup();
    await press(radio(NOT_KEPT));
    await press(radio(/^ChatGPT/));
    expect(visibleText()).toContain(
      "OpenAI doesn't describe how it treats requests that your plan pays for.",
    );
  });

  test("ChatGPT not eligible: the account has no plan that counts", async () => {
    await renderSetup();
    await press(radio(FREE));
    await press(radio(/^ChatGPT/));
    signInStarts();
    await press(button(CONTINUE_CHATGPT)!);
    await act(async () => {
      emitEvent("postProcessSignInFinished", {
        provider: "chatgpt",
        ok: false,
        error: "not_eligible",
        free_tier: null,
      });
    });
    await settle();
    expect(visibleText()).toContain(
      "This ChatGPT account has no plan that DanyWhere can use",
    );
  });
});

// A failed sign-in doesn't hold up the rest of the setup (sidecar qa
// 2026-11-26_2250, D, H2): another answer, another way and Later work.
describe("post-processing setup: after a failed sign-in", () => {
  const chatgptFails = async (
    status: SetupStatus | (() => SetupStatus) = notReady(),
  ) => {
    await renderSetup(status);
    await press(radio(FREE));
    await press(radio(/^ChatGPT/));
    signInStarts();
    await press(button(CONTINUE_CHATGPT)!);
    await act(async () => {
      emitEvent("postProcessSignInFinished", {
        provider: "chatgpt",
        ok: false,
        error: "not_eligible",
        free_tier: null,
      });
    });
    await settle();
    expect(visibleText()).toContain(
      "This ChatGPT account has no plan that DanyWhere can use",
    );
  };

  test("another answer leads to another sign-in, which can start", async () => {
    await chatgptFails();
    await press(radio(NONE));

    expect(visibleText()).not.toContain("This ChatGPT account has no plan");
    const connect = button(CONNECT_OPENROUTER)!;
    expect(connect.hasAttribute("disabled")).toBe(false);
    await press(connect);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE, null],
      ["openrouter", BROWSER_PAGE, "free"],
    ]);
    expect(visibleText()).toContain("Waiting for the browser …");
  });

  test("a guided key still leads to the card", async () => {
    let status = notReady({ provider_id: "anthropic" });
    await chatgptFails(() => status);
    await press(radio(/^Claude/));
    mockCommand("checkPostProcessKey", async () =>
      ok({ models: ["claude-haiku-5-5"], recommended: "claude-haiku-5-5" }),
    );
    mockCommand("applyPostProcessChoice", async () => {
      status = ready({
        provider_id: "anthropic",
        model: "claude-haiku-5-5",
        way: { kind: "guided_key", provider: "anthropic" },
      });
      return ok(null);
    });
    await typeInto(screen.getByLabelText("Key from Anthropic"), "sk-ant-test");
    await press(button("Check")!);

    expect(visibleText()).toContain("Connected to Anthropic");
  });

  test("Later still puts it off", async () => {
    await chatgptFails();
    await press(button("Later")!);
    expect(visibleText()).toContain("Post-processing isn't set up yet.");
    expect(visibleText()).not.toContain(Q1);
  });
});

// "Sign in again" from a notice starts a sign-in that the answers may not
// lead to, e.g. ChatGPT chosen under All providers (D, M6): the page shows
// its waiting all the same.
describe("post-processing setup: a sign-in the answers don't lead to", () => {
  const signInFromNotice = async () => {
    await renderSetup(
      notReady({ provider_id: "chatgpt", readiness: "sign_in" }),
      {
        texts_may_go: "free_services",
        subscription: "none",
      },
    );
    signInStarts();
    await act(async () => {
      runFailureAction({ kind: "sign_in_again", provider: "chatgpt" });
    });
    await settle();
  };

  test("waits with Open the page again and Cancel", async () => {
    await signInFromNotice();

    const text = visibleText();
    expect(text).toContain("Sign-in with ChatGPT");
    expect(text).toContain("Waiting for the browser …");
    expect(
      screen.getAllByRole("button", { name: "Open the page again" }),
    ).toHaveLength(1);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE, null],
    ]);
    await press(button("Cancel")!);
    expect(callsOf("cancelPostProcessSignIn")).toEqual([[]]);
    expect(visibleText()).not.toContain("Waiting for the browser …");
    expect(rawTextIn()).toEqual([]);
  });

  test("a failed one says why and can start again", async () => {
    await signInFromNotice();
    await act(async () => {
      emitEvent("postProcessSignInFinished", {
        provider: "chatgpt",
        ok: false,
        error: "denied",
        free_tier: null,
      });
    });
    await settle();
    expect(visibleText()).toContain("Access wasn't allowed.");
    await press(button("Try again")!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE, null],
      ["chatgpt", BROWSER_PAGE, null],
    ]);
  });
});

describe("post-processing setup: a guided key", () => {
  const toClaude = async () => {
    await renderSetup();
    await press(radio(NOT_KEPT));
    await press(radio(/^Claude/));
  };

  test("steps with links, a field, a check button, then the sample", async () => {
    let status = notReady({ provider_id: "anthropic" });
    answerStatus(() => status);
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null),
      scope: DEFAULTS_SCOPE,
    });
    await press(radio(NOT_KEPT));
    await press(radio(/^Claude/));

    const text = visibleText();
    expect(text).toContain("Suggestion: a key from Anthropic");
    expect(text).toContain("Link your plan's API credits in claude.ai.");
    expect(text).toContain("you don't need to add a card");
    // A key of another organization has none of the plan's credits.
    expect(text).toContain(
      "Create a key in the Claude Console, in the organization you linked.",
    );
    await press(button("Open the Console")!);
    expect(openedUrls()).toEqual(["https://platform.claude.com/settings/keys"]);

    const check = button("Check")!;
    expect(check.hasAttribute("disabled")).toBe(true);
    mockCommand("checkPostProcessKey", async () =>
      ok({
        models: ["claude-haiku-5-5", "claude-sonnet-5-5"],
        recommended: "claude-haiku-5-5",
      }),
    );
    mockCommand("applyPostProcessChoice", async () => {
      status = ready({
        provider_id: "anthropic",
        model: "claude-haiku-5-5",
        way: { kind: "guided_key", provider: "anthropic" },
      });
      return ok(null);
    });
    mockCommand("runPostProcessSample", async () =>
      ok({
        output: "So I'll send you that tomorrow, okay?",
        model: "claude-haiku-5-5",
        millis: 640,
      }),
    );
    await typeInto(
      screen.getByLabelText("Key from Anthropic"),
      "  sk-ant-test  ",
    );
    await press(button("Check")!);

    expect(callsOf("checkPostProcessKey")).toEqual([
      ["anthropic", "sk-ant-test"],
    ]);
    expect(callsOf("applyPostProcessChoice")).toEqual([
      [
        {
          provider_id: "anthropic",
          model: "claude-haiku-5-5",
          api_key: "sk-ant-test",
          effort: null,
        },
      ],
    ]);
    expect(callsOf("runPostProcessSample")).toEqual([[SAMPLE_TEXT]]);
    const after = visibleText();
    expect(after).toContain("Connected to Anthropic");
    expect(after).toContain("Model: claude-haiku-5-5");
    expect(after).toContain("Result: “So I'll send you that tomorrow, okay?”");
    expect(lastAnswers()?.finished).toBe(true);
    expect(rawTextIn()).toEqual([]);
  });

  test("a check that times out says how long it waited", async () => {
    await toClaude();
    mockCommand("checkPostProcessKey", async () =>
      failed("timeout", {
        providerId: "anthropic",
        provider: "Anthropic",
        seconds: 30,
      }),
    );
    await typeInto(screen.getByLabelText("Key from Anthropic"), "sk-ant-slow");
    await press(button("Check")!);
    expect(visibleText()).toContain(
      "Anthropic didn't answer within the time limit (30 s).",
    );
    expect(visibleText()).not.toContain("{");
  });

  test("a refused key says why and keeps nothing", async () => {
    await toClaude();
    mockCommand("checkPostProcessKey", async () => failed("key-refused"));
    await typeInto(screen.getByLabelText("Key from Anthropic"), "sk-ant-wrong");
    await press(button("Check")!);
    expect(visibleText()).toContain("Anthropic refused the API key.");
    expect(callsOf("applyPostProcessChoice")).toEqual([]);
  });

  test("Hugging Face: a token with the permission for Inference Providers", async () => {
    await renderSetup();
    await press(radio(FREE));
    await press(radio(/^Hugging Face/));
    expect(visibleText()).toContain("“Make calls to Inference Providers”");
    expect(screen.queryByLabelText("Key from Hugging Face")).not.toBeNull();
  });
});

describe("post-processing setup: connected", () => {
  test("the card: try it, credit, disconnect, Advanced", async () => {
    mockCommand("runPostProcessSample", async () =>
      ok({
        output: "So I'll send you that tomorrow, okay?",
        model: "nvidia/nemotron-3.5-super-vl:free",
        millis: 7300,
      }),
    );
    await renderSetup(ready(), {
      texts_may_go: "free_services",
      subscription: "none",
    });

    const text = visibleText();
    expect(text).toContain("Connected to OpenRouter");
    expect(text).not.toContain(Q1);
    // Today's provider settings wait under Advanced.
    expect(text).not.toContain("API Key");
    await press(button("Try it")!);
    expect(callsOf("runPostProcessSample")).toEqual([[SAMPLE_TEXT]]);
    expect(visibleText()).toContain(
      "7.3 s with nvidia/nemotron-3.5-super-vl:free",
    );

    await press(button("Credit")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/credits"]);

    await press(button("Advanced")!);
    expect(visibleText()).toContain("API Key");

    // Disconnect forgets the key here only (D, N16).
    expect(visibleText()).toContain(
      "OpenRouter keeps the key “DanyWhere” among your API keys until you delete it there.",
    );
    await press(button("Disconnect")!);
    expect(callsOf("disconnectPostProcessSignIn")).toEqual([["openrouter"]]);
    expect(rawTextIn()).toEqual([]);
  });

  test("a sample that fails names the reason", async () => {
    mockCommand("runPostProcessSample", async () => failed("free-busy"));
    await renderSetup(ready());
    await press(button("Try it")!);
    const text = visibleText();
    expect(text).toContain("The sample didn't work.");
    expect(text).toContain("The free models at OpenRouter are busy right now");
  });

  // The reason's own values come in the failure's detail (D, M4).
  test("a sample that fails with a status says it, not the raw detail", async () => {
    mockCommand("runPostProcessSample", async () =>
      failed("status", {
        providerId: "openrouter",
        provider: "OpenRouter",
        status: 503,
      }),
    );
    await renderSetup(ready());
    await press(button("Try it")!);
    const text = visibleText();
    expect(text).toContain("OpenRouter answered with HTTP status 503.");
    expect(text).not.toContain("{");
  });

  test("a sample that fails otherwise shows the error, not the raw detail", async () => {
    mockCommand("runPostProcessSample", async () =>
      failed("other", {
        providerId: "openrouter",
        provider: "OpenRouter",
        error: "connection reset",
      }),
    );
    await renderSetup(ready());
    await press(button("Try it")!);
    expect(visibleText()).toContain("Error: connection reset");
    expect(visibleText()).not.toContain("{");
  });

  test("OpenRouter with credit, not topped up yet: Top up and I've topped up", async () => {
    mockCommand("refreshOpenrouterConnection", async () =>
      ok({
        free_tier: true,
        limit_usd: 10,
        free_models: [],
        checked_at: 1795300000,
      }),
    );
    await renderSetup(ready(), {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "credit",
    });
    expect(visibleText()).toContain(
      "No credit yet: DanyWhere sends nothing until you top up.",
    );
    expect(visibleText()).not.toContain("free models");
    await press(button("Top up")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/credits"]);
    await press(button("I've topped up")!);
    expect(callsOf("refreshOpenrouterConnection")).toEqual([[]]);
    expect(visibleText()).toContain("OpenRouter doesn't show any credit yet.");
  });

  // The free way keeps to the free models on an account that has credit
  // (D, M1); the way with credit names its model once there is credit.
  test("the free way on an account with credit still says free models", async () => {
    await renderSetup(
      ready({ way: { kind: "openrouter", free_tier: false, limit_usd: 10 } }),
      { texts_may_go: "free_services", subscription: "none" },
    );
    const text = visibleText();
    expect(text).toContain("Free models, up to 50 dictations a day");
    expect(text).not.toContain("With credit");
  });

  test("the way with credit, topped up: a model without training", async () => {
    await renderSetup(
      ready({ way: { kind: "openrouter", free_tier: false, limit_usd: 10 } }),
      { texts_may_go: "not_kept", subscription: "none", next_step: "credit" },
    );
    const text = visibleText();
    expect(text).toContain("With credit, a model without training");
    expect(text).not.toContain("Free models");
  });

  test("ChatGPT: Using ChatGPT plan, and Manage usage", async () => {
    await renderSetup(
      ready({
        provider_id: "chatgpt",
        model: "gpt-6.2-sol",
        way: { kind: "chatgpt_plan", plan: "Plus", email: null },
      }),
    );
    const text = visibleText();
    expect(text).toContain("Connected to ChatGPT");
    expect(text).toContain("Using ChatGPT plan");
    expect(text).toContain("Plan: Plus");
    await press(button("Manage usage")!);
    expect(openedUrls()).toEqual(["https://chatgpt.com/settings/usage"]);
    await press(button("Disconnect")!);
    expect(callsOf("disconnectPostProcessSignIn")).toEqual([["chatgpt"]]);
  });

  // OpenAI's guidelines: when revoking fails, say so and where the user can
  // check the access (sidecar qa 2026-11-26_2250, D, Nachtrag N).
  test("ChatGPT: a sign-out OpenAI didn't confirm says so, with its settings", async () => {
    mockCommand("disconnectPostProcessSignIn", async () =>
      ok({ revoke_confirmed: false }),
    );
    await renderSetup(
      ready({
        provider_id: "chatgpt",
        model: "gpt-6.2-sol",
        way: { kind: "chatgpt_plan", plan: null, email: null },
      }),
    );
    await press(button("Disconnect")!);
    expect(raisedNotices()).toEqual([
      "Sign-Out Not Confirmed by OpenAI | DanyWhere has forgotten the sign-in here. You can check DanyWhere's access in your ChatGPT settings.",
    ]);
    const notice = useNoticeStore.getState().local[0];
    expect(notice.kept).toBe(true);
    expect(notice.action?.label).toBe("ChatGPT Settings");
    await act(async () => notice.action!.onClick());
    expect(openedUrls()).toEqual(["https://chatgpt.com/settings/usage"]);
  });

  test("ChatGPT: a confirmed sign-out raises nothing", async () => {
    for (const confirmed of [true, null]) {
      mockCommand("disconnectPostProcessSignIn", async () =>
        ok({ revoke_confirmed: confirmed }),
      );
      await renderSetup(
        ready({
          provider_id: "chatgpt",
          model: "gpt-6.2-sol",
          way: { kind: "chatgpt_plan", plan: null, email: null },
        }),
      );
      await press(button("Disconnect")!);
      expect(raisedNotices()).toEqual([]);
      cleanup();
    }
  });

  // A failure is said, not swallowed (D, N8).
  test("a Disconnect that fails says so", async () => {
    mockCommand("disconnectPostProcessSignIn", async () =>
      err("the vault is locked"),
    );
    await renderSetup(ready());
    await press(button("Disconnect")!);
    expect(raisedNotices()).toEqual(["Not Disconnected | the vault is locked"]);
  });

  test("Open the page again that fails says so", async () => {
    await renderSetup();
    await press(radio(FREE));
    await press(radio(NONE));
    signInStarts();
    await press(button(CONNECT_OPENROUTER)!);
    mockCommand("reopenPostProcessSignIn", async () =>
      err("no sign-in is waiting"),
    );
    await press(button("Open the page again")!);
    expect(raisedNotices()).toEqual([
      "Page Not Opened Again | no sign-in is waiting",
    ]);
  });

  test("I've topped up that can't reach OpenRouter says why", async () => {
    mockCommand("refreshOpenrouterConnection", async () =>
      failed("unreachable", {
        providerId: "openrouter",
        provider: "OpenRouter",
      }),
    );
    await renderSetup(ready(), {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "credit",
    });
    await press(button("I've topped up")!);
    const text = visibleText();
    expect(text).toContain(
      "Couldn't ask OpenRouter about the credit. OpenRouter couldn't be reached.",
    );
    expect(text).not.toContain("OpenRouter doesn't show any credit yet.");
  });

  test("a status the backend can't give leaves the questions", async () => {
    mockCommand("getPostProcessSetupStatus", async () => {
      throw new Error("the backend is gone");
    });
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null),
      scope: DEFAULTS_SCOPE,
    });
    expect(visibleText()).toContain(Q1);
  });

  // The card shows for any provider that is ready, also with a key the user
  // pasted long before the setup, so removing it asks a second click (D,
  // N14).
  test("a key of one's own: Disconnect removes the key after a second click", async () => {
    await renderSetup(
      ready({
        provider_id: "anthropic",
        model: "claude-haiku-5-5",
        way: { kind: "guided_key", provider: "anthropic" },
      }),
    );
    expect(visibleText()).toContain("Connected to Anthropic");
    await press(button("Disconnect")!);
    expect(callsOf("applyPostProcessChoice")).toEqual([]);
    expect(button("Disconnect")).toBeNull();
    await press(button("Click again to remove the key")!);
    expect(callsOf("applyPostProcessChoice")).toEqual([
      [{ provider_id: "anthropic", model: null, api_key: "", effort: null }],
    ]);
  });

  test("a key of one's own: the second click is asked again after leaving the button", async () => {
    await renderSetup(
      ready({
        provider_id: "anthropic",
        model: "claude-haiku-5-5",
        way: { kind: "guided_key", provider: "anthropic" },
      }),
    );
    await press(button("Disconnect")!);
    await act(async () => {
      fireEvent.blur(button("Click again to remove the key")!);
    });
    expect(button("Disconnect")).not.toBeNull();
    expect(callsOf("applyPostProcessChoice")).toEqual([]);
  });

  test("a server of one's own has nothing to disconnect", async () => {
    await renderSetup(
      ready({
        provider_id: "custom",
        model: "llama3.2",
        way: { kind: "own_server" },
      }),
    );
    expect(visibleText()).toContain("Model: llama3.2");
    expect(button("Try it")).not.toBeNull();
    expect(button("Disconnect")).toBeNull();
  });

  test("Answer the questions again brings them back with the answers kept", async () => {
    await renderSetup(ready(), {
      texts_may_go: "free_services",
      subscription: "none",
    });
    await press(button("Answer the questions again")!);
    expect(visibleText()).toContain(Q1);
    expect(radio(FREE).getAttribute("aria-checked")).toBe("true");
    expect(radio(NONE).getAttribute("aria-checked")).toBe("true");
    await press(button("Keep the current setup")!);
    expect(visibleText()).toContain("Connected to OpenRouter");
  });

  // The answers describe the way in use, so keeping it takes back what was
  // answered meanwhile (D, M6).
  test("Keep the current setup takes back the answers given meanwhile", async () => {
    await renderSetup(ready(), {
      texts_may_go: "free_services",
      subscription: "none",
    });
    await press(button("Answer the questions again")!);
    await press(radio(NOT_KEPT));
    expect(lastAnswers()?.answers?.texts_may_go).toBe("not_kept");

    await press(button("Keep the current setup")!);
    expect(lastAnswers()).toEqual({
      answers: {
        texts_may_go: "free_services",
        subscription: "none",
        next_step: null,
      },
      finished: true,
      skipped: false,
    });
    await press(button("Answer the questions again")!);
    expect(radio(FREE).getAttribute("aria-checked")).toBe("true");
  });
});

describe("post-processing setup in German", () => {
  test("the questions and the buttons in plain words", async () => {
    await switchTestLanguage("de");
    await renderSetup();
    expect(visibleText()).toContain("Texte verbessern lassen");
    expect(visibleText()).toContain("1. Wohin dürfen deine Texte?");
    await press(radio(/^Auch zu Gratisdiensten\./));
    expect(visibleText()).toContain("2. Hast du schon eines dieser Abos?");
    expect(radio(/^ChatGPT/).textContent).toBe(
      "ChatGPT Go, Plus, Pro oder Business",
    );
    await press(radio(/^Keines davon/));
    expect(visibleText()).toContain("Vorschlag: OpenRouter, gratis");
    expect(button("Mit OpenRouter verbinden")).not.toBeNull();
    await press(radio(/^ChatGPT/));
    expect(button("Mit ChatGPT fortfahren")).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
    // The browser's texts after the callback are German too.
    signInStarts();
    await press(button("Mit ChatGPT fortfahren")!);
    expect(callsOf("startPostProcessSignIn")).toEqual([
      ["chatgpt", BROWSER_PAGE_DE, null],
    ]);
  });

  test("the card says Nutzung verwalten", async () => {
    await switchTestLanguage("de");
    await renderSetup(
      ready({
        provider_id: "chatgpt",
        model: "gpt-6.2-sol",
        way: { kind: "chatgpt_plan", plan: "Plus", email: null },
      }),
    );
    expect(visibleText()).toContain("Läuft über dein ChatGPT-Abo");
    expect(button("Nutzung verwalten")).not.toBeNull();
    expect(button("Ausprobieren")).not.toBeNull();
    expect(button("Trennen")).not.toBeNull();
    expect(rawTextIn()).toEqual([]);
  });
});

// What happens while the page isn't connected yet but shows nothing wrong.
describe("post-processing setup: the page around it", () => {
  test("without an answer from the backend the questions still come", async () => {
    mockCommand(
      "getPostProcessSetupStatus",
      async () => null as unknown as SetupStatus,
    );
    await renderPage(<PostProcessingSettings />, {
      settings: setupSettings(null),
      scope: DEFAULTS_SCOPE,
    });
    expect(visibleText()).toContain(Q1);
    expect(isVisible(screen.getByText(Q1))).toBe(true);
  });

  test("a “?” next to Improve Your Texts opens the manual's chapter", async () => {
    answerStatus(notReady());
    const opened: (string | undefined)[] = [];
    await renderPage(
      <OpenManualProvider value={(page) => opened.push(page)}>
        <PostProcessingSettings />
      </OpenManualProvider>,
      { settings: setupSettings(null), scope: DEFAULTS_SCOPE },
    );
    const help = screen.getByRole("button", {
      name: "Open the manual: Setting Up Post-Processing",
    });
    const title = screen.getByRole("heading", { name: TITLE });
    expect(title.parentElement?.contains(help)).toBe(true);
    await press(help);
    expect(opened).toEqual(["how-it-works/post-processing/"]);
    expect(rawTextIn()).toEqual([]);
  });
});
