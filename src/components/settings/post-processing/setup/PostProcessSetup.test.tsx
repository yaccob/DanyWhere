// DanyWhere: the post-processing setup at the top of the page "Post Process"
// (sidecar qa 2026-11-26_1840, sections 4 to 6, and 2026-11-26_2250, 4.5).
// Questions anyone can answer come one after the other, the suggestion
// under them changes with the answers, and its button sets the way up: a
// sign-in through the browser (OpenRouter, ChatGPT), a guided key, or a
// server of one's own. Once set up, a card says what it is connected to and
// tries a sample sentence; today's provider settings wait under "Advanced"
// or "All providers …".
import { beforeEach, describe, expect, test } from "bun:test";
import { act, fireEvent, screen } from "@testing-library/react";
import { PostProcessingSettings } from "../PostProcessingSettings";
import { DEFAULTS_SCOPE } from "@/lib/profileScope";
import { callsOf, emitEvent, mockCommand, ok } from "@/test/bindings-mock";
import { switchTestLanguage } from "@/test/i18n";
import { press, renderPage, setBackendSettings, settle } from "@/test/render";
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
      ["openrouter", BROWSER_PAGE],
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
    await renderSetup(notReady({ memory_bytes: 8 * GiB }));
    await press(radio(NOWHERE));

    const text = visibleText();
    expect(text).not.toContain(Q2);
    expect(text).toContain("Not possible on this computer");
    expect(text).toContain("this Mac has 8 GB");
    // The backend makes the custom provider the defaults' one.
    mockCommand("applyPostProcessChoice", async () => {
      setBackendSettings(
        setupSettings(null, {
          defaults: { post_process_provider_id: "custom" },
        }),
      );
      return ok(null);
    });
    await press(button("Set up a server of your own")!);
    expect(callsOf("applyPostProcessChoice")).toEqual([
      [{ provider_id: "custom", model: null, api_key: null, effort: null }],
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
      ["openrouter", BROWSER_PAGE],
      ["openrouter", BROWSER_PAGE],
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
      ["chatgpt", BROWSER_PAGE],
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
      "No credit yet: DanyWhere uses the free models until you top up.",
    );
    await press(button("Top up")!);
    expect(openedUrls()).toEqual(["https://openrouter.ai/settings/credits"]);
    await press(button("I've topped up")!);
    expect(callsOf("refreshOpenrouterConnection")).toEqual([[]]);
    expect(visibleText()).toContain("OpenRouter doesn't show any credit yet.");
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

  test("a key of one's own: Disconnect removes the key", async () => {
    await renderSetup(
      ready({
        provider_id: "anthropic",
        model: "claude-haiku-5-5",
        way: { kind: "guided_key", provider: "anthropic" },
      }),
    );
    expect(visibleText()).toContain("Connected to Anthropic");
    await press(button("Disconnect")!);
    expect(callsOf("applyPostProcessChoice")).toEqual([
      [{ provider_id: "anthropic", model: null, api_key: "", effort: null }],
    ]);
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
      ["chatgpt", BROWSER_PAGE_DE],
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
});
