// DanyWhere: building blocks for the tests of the post-processing setup
// (sidecar qa 2026-11-26_2250, strand U): what the backend says about the
// setup, the answers it keeps, the pages a click opened in the browser, and
// a system that asks for less motion.
import type {
  AppSettings,
  PostProcessProvider,
  SetupAnswers,
  SetupStatus,
} from "@/bindings";
import type { SetupAnswers as GivenAnswers } from "@/lib/postProcessSetup/questions";
import { callsOf, mockCommand, ok } from "./bindings-mock";
import { ipcCalls } from "./tauri-mock";
import { settingsWith, type SettingsSpec } from "./fixtures";

export const GiB = 1024 ** 3;

/** The backend's status of a setup that isn't done: no provider is ready. */
export const notReady = (status: Partial<SetupStatus> = {}): SetupStatus => ({
  readiness: "key",
  provider_id: "openai",
  model: null,
  way: null,
  memory_bytes: 32 * GiB,
  setup: { answers: null, finished: false, skipped: false },
  ...status,
});

/** The backend's status once a way is set up. */
export const ready = (status: Partial<SetupStatus> = {}): SetupStatus => ({
  readiness: "ready",
  provider_id: "openrouter",
  model: "nvidia/nemotron-3.5-super-vl:free",
  way: { kind: "openrouter", free_tier: true, limit_usd: 10 },
  memory_bytes: 32 * GiB,
  setup: { answers: null, finished: true, skipped: false },
  ...status,
});

/** `get_post_process_setup_status` answers `status`, or what `status()`
 * gives at the time of the call, so a test can change it. */
export const answerStatus = (status: SetupStatus | (() => SetupStatus)) =>
  mockCommand("getPostProcessSetupStatus", async () =>
    typeof status === "function" ? status() : status,
  );

/** The providers of the defaults as the backend lists them. */
export const PROVIDERS: PostProcessProvider[] = [
  { id: "openai", label: "OpenAI", base_url: "https://api.openai.com/v1" },
  {
    id: "openrouter",
    label: "OpenRouter",
    base_url: "https://openrouter.ai/api/v1",
  },
  {
    id: "anthropic",
    label: "Anthropic",
    base_url: "https://api.anthropic.com/v1",
    api_format: "anthropic",
  },
  { id: "groq", label: "Groq", base_url: "https://api.groq.com/openai/v1" },
  {
    id: "chatgpt",
    label: "ChatGPT",
    base_url: "https://api.openai.com/v1",
  },
  {
    id: "gemini",
    label: "Google Gemini",
    base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
  },
  {
    id: "huggingface",
    label: "Hugging Face",
    base_url: "https://router.huggingface.co/v1",
  },
  {
    id: "ollama_cloud",
    label: "Ollama Cloud",
    base_url: "https://ollama.com/v1",
  },
  {
    id: "custom",
    label: "Custom",
    base_url: "http://localhost:11434/v1",
    allow_base_url_edit: true,
  },
];

/** Settings with the providers above, and the setup's answers as kept:
 * every field, `null` where unanswered, as the backend writes them. */
export const setupSettings = (
  answers: GivenAnswers | null = null,
  spec: SettingsSpec = {},
  kept: { finished?: boolean; skipped?: boolean } = {},
): AppSettings =>
  settingsWith({
    ...spec,
    defaults: { post_process_providers: PROVIDERS, ...spec.defaults },
    app: {
      ...spec.app,
      post_process_setup: {
        answers: answers && {
          texts_may_go: null,
          subscription: null,
          next_step: null,
          ...answers,
        },
        finished: kept.finished ?? false,
        skipped: kept.skipped ?? false,
      },
    },
  });

/** The answers of the last `set_post_process_setup_answers`, with its two
 * flags, or null when none was sent. */
export const lastAnswers = (): {
  answers: SetupAnswers | null;
  finished: boolean;
  skipped: boolean;
} | null => {
  const calls = callsOf("setPostProcessSetupAnswers");
  const last = calls[calls.length - 1];
  return last
    ? { answers: last[0], finished: last[1], skipped: last[2] }
    : null;
};

/** A command's failure as the backend sends it (`FailureDto`): the reason
 * and, as its detail, the notice's JSON object with the reason and `fields`
 * (`providerId`, `provider`, `status`, `seconds`, `error` …), as
 * `PostProcessFailure::notice_detail` writes it. */
export const failed = (
  reason: string,
  fields: Record<string, string | number> = {},
) => ({
  status: "error" as const,
  error: { reason, detail: JSON.stringify({ reason, ...fields }) },
});

/** The pages a click opened in the browser, in order. */
export const openedUrls = (): string[] =>
  ipcCalls()
    .filter((call) => call.cmd === "plugin:opener|open_url")
    .map((call) => (call.args as { url: string }).url);

/** The texts the browser shows after the sign-in's callback, in English. */
export const BROWSER_PAGE = {
  success: "Done. You can close this window and go back to DanyWhere.",
  failure: "That didn't work. Go back to DanyWhere and try again.",
};

/** The same in German. */
export const BROWSER_PAGE_DE = {
  success:
    "Fertig. Du kannst dieses Fenster schließen und zu DanyWhere zurückkehren.",
  failure:
    "Das hat nicht geklappt. Geh zurück zu DanyWhere und versuch es noch einmal.",
};

/** Starting a sign-in works and gives this page. */
export const signInStarts = () =>
  mockCommand("startPostProcessSignIn", async (provider) =>
    ok({ auth_url: `https://${provider}.example/auth` }),
  );

/** The system asks for less motion until the next reset (`matchMedia`). */
export const askForLessMotion = (): (() => void) => {
  const original = window.matchMedia;
  window.matchMedia = ((query: string) => ({
    matches: query.includes("prefers-reduced-motion"),
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
  return () => {
    window.matchMedia = original;
  };
};
