// DanyWhere: the guided keys of the post-processing setup (sidecar qa
// 2026-11-26_1840, section 3, and 2026-11-26_1955, section 4): for each
// provider whose way is a key to paste, the steps that lead to it, each with
// the page it needs; and the pages of a provider's account where usage,
// credit and keys are managed, for the setup's card and the notices.
import type { GuidedKeyProvider } from "./questions";

const KEY = "settings.postProcessing.setup.guides";

export interface GuideStep {
  /** The step's sentence (a translation key). */
  text: string;
  /** The page the step needs: its button's label (a translation key) and
   * address. */
  link?: { label: string; url: string };
}

export interface Guide {
  /** The post-processing provider the key is for. */
  provider: GuidedKeyProvider;
  /** Where the key comes from: a product name, not translated. */
  service: string;
  steps: GuideStep[];
}

export const GUIDES: Record<GuidedKeyProvider, Guide> = {
  // Claude Max, Ultra and Team: the plan's monthly API credits on a linked
  // Console organization (support.claude.com, article 17154008).
  anthropic: {
    provider: "anthropic",
    service: "Anthropic",
    steps: [
      {
        text: `${KEY}.anthropic.credits`,
        link: {
          label: `${KEY}.anthropic.creditsLink`,
          url: "https://support.claude.com/en/articles/17154008-monthly-api-credits-for-max-ultra-and-team-plans",
        },
      },
      {
        text: `${KEY}.anthropic.key`,
        link: {
          label: `${KEY}.anthropic.keyLink`,
          url: "https://platform.claude.com/settings/keys",
        },
      },
      { text: `${KEY}.copyKey` },
    ],
  },
  // Google AI Pro and Ultra: Cloud credits for the Gemini API after a
  // prepayment of at least 5 USD (aistudio.google.com/docs/google-ai-plans).
  gemini: {
    provider: "gemini",
    service: "Google AI Studio",
    steps: [
      {
        text: `${KEY}.gemini.credits`,
        link: {
          label: `${KEY}.gemini.creditsLink`,
          url: "https://aistudio.google.com/",
        },
      },
      {
        text: `${KEY}.gemini.key`,
        link: {
          label: `${KEY}.gemini.keyLink`,
          url: "https://aistudio.google.com/apikey",
        },
      },
      { text: `${KEY}.copyKey` },
    ],
  },
  // Hugging Face PRO: a token first, signing in with OAuth later (it needs
  // an app registered at Hugging Face; qa 2026-11-26_2250, section 2).
  huggingface: {
    provider: "huggingface",
    service: "Hugging Face",
    steps: [
      {
        text: `${KEY}.huggingface.key`,
        link: {
          label: `${KEY}.huggingface.keyLink`,
          // A new fine-grained token with the permission already set
          // (huggingface.co/docs/inference-providers, "Authentication").
          url: "https://huggingface.co/settings/tokens/new?ownUserPermissions=inference.serverless.write&tokenType=fineGrained",
        },
      },
      { text: `${KEY}.copyToken` },
    ],
  },
  // Ollama Pro and Max: usage credits, OpenAI-compatible at ollama.com/v1.
  ollama_cloud: {
    provider: "ollama_cloud",
    service: "Ollama",
    steps: [
      {
        text: `${KEY}.ollama_cloud.key`,
        link: {
          label: `${KEY}.ollama_cloud.keyLink`,
          url: "https://ollama.com/settings/keys",
        },
      },
      { text: `${KEY}.copyKey` },
    ],
  },
  // Groq's free tier: an account through Google without a card; creating
  // the key asks for a Cloudflare check (trial of 2026-11-26).
  groq: {
    provider: "groq",
    service: "Groq",
    steps: [
      {
        text: `${KEY}.groq.account`,
        link: {
          label: `${KEY}.groq.accountLink`,
          url: "https://console.groq.com/keys",
        },
      },
      { text: `${KEY}.groq.key` },
      { text: `${KEY}.copyKey` },
    ],
  },
};

/** The guide of a provider, or none when its way isn't a guided key. */
export const guideFor = (providerId: string): Guide | null =>
  (GUIDES as Record<string, Guide | undefined>)[providerId] ?? null;

/** Where ChatGPT's plan usage is managed (OpenAI's UI guidelines for Sign
 * in with ChatGPT: "Manage usage" goes there). */
export const CHATGPT_USAGE_URL = "https://chatgpt.com/settings/usage";

/** Pages of a provider's account. */
export interface ProviderPages {
  /** Where its usage is seen and limited. */
  usage?: string;
  /** Where credit is topped up. */
  credit?: string;
  /** Where its keys are, and at OpenRouter their limits. */
  keys?: string;
}

export const PROVIDER_PAGES: Record<string, ProviderPages> = {
  chatgpt: { usage: CHATGPT_USAGE_URL },
  openrouter: {
    credit: "https://openrouter.ai/settings/credits",
    keys: "https://openrouter.ai/settings/keys",
  },
  anthropic: {
    usage: "https://platform.claude.com/settings/spend-limits",
    credit: "https://platform.claude.com/settings/billing",
    keys: "https://platform.claude.com/settings/keys",
  },
  // Requests and costs, and the credits with their expiry
  // (aistudio.google.com/docs/google-ai-plans).
  gemini: {
    usage: "https://aistudio.google.com/usage",
    credit: "https://aistudio.google.com/billing",
    keys: "https://aistudio.google.com/apikey",
  },
  huggingface: {
    usage: "https://huggingface.co/settings/billing",
    credit: "https://huggingface.co/settings/billing",
    keys: "https://huggingface.co/settings/tokens",
  },
  ollama_cloud: {
    usage: "https://ollama.com/settings",
    keys: "https://ollama.com/settings/keys",
  },
  // "Dashboard, Usage" (console.groq.com/docs/billing-faqs).
  groq: {
    usage: "https://console.groq.com/dashboard/usage",
    keys: "https://console.groq.com/keys",
  },
};

export const providerPages = (providerId: string): ProviderPages =>
  PROVIDER_PAGES[providerId] ?? {};
