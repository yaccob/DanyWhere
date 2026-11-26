// The questions that set up post-processing, as pure logic: the user's
// answers (and this computer) in, the next question and a suggested way out.
// No texts here; the page turns ids and ways into words.
//
// 1. "Where may your texts go?"
// 2. "Do you already have one of these subscriptions?" (not after "nowhere")
// 3. "How should it go on?" (only after "not kept" and no subscription)
//
// No provider is preferred: subscriptions are listed alphabetically, and the
// local model comes last in every choice and only from 16 GiB of memory.

/** Answer to question 1. */
export type TextsMayGo = "free_services" | "not_kept" | "nowhere";
/** Answer to question 2; each id stands for the plans that include use in other apps. */
export type Subscription =
  | "chatgpt"
  | "claude"
  | "google"
  | "huggingface"
  | "ollama"
  | "none";
/** Answer to question 3. */
export type NextStep = "credit" | "groq" | "this_computer";

/** The answers as the settings store keeps them (serde's snake_case names). */
export interface SetupAnswers {
  texts_may_go?: TextsMayGo | null;
  subscription?: Subscription | null;
  next_step?: NextStep | null;
}

export interface Machine {
  memoryBytes: number;
  platform: "macos" | "windows" | "linux";
}

/** The providers a guided key sets up (ids of the post-processing providers). */
export type GuidedKeyProvider =
  | "anthropic"
  | "gemini"
  | "huggingface"
  | "ollama_cloud"
  | "groq";

export type Way =
  | { kind: "openrouter_free" }
  | { kind: "openrouter_credit" }
  /** `dataNotDescribed`: the texts mustn't be kept, and OpenAI doesn't describe how it treats requests paid by the plan. */
  | { kind: "chatgpt_plan"; dataNotDescribed: boolean }
  | { kind: "guided_key"; provider: GuidedKeyProvider }
  /** `available` is false until the local model is built: the page says it comes later and offers a server of one's own. */
  | { kind: "local_model"; available: boolean }
  /** "Nowhere" on a computer with too little memory for the local model. */
  | { kind: "nowhere_unavailable" };

/** The local model is offered from this much memory. */
export const LOCAL_MODEL_MIN_BYTES = 16 * 1024 ** 3;

/** Whether the local model is built yet (stage 2 of the setup). */
const LOCAL_MODEL_BUILT = false;

const TEXTS_MAY_GO: readonly TextsMayGo[] = [
  "free_services",
  "not_kept",
  "nowhere",
];
const NEXT_STEPS: readonly NextStep[] = ["credit", "groq", "this_computer"];

/**
 * Subscriptions that pay for requests from other apps, alphabetically by
 * provider, each with the plans that count; "none" last. Plan names are
 * product names and aren't translated.
 */
const SUBSCRIPTION_OPTIONS: ReadonlyArray<{
  id: Subscription;
  plans: readonly string[];
}> = [
  {
    id: "chatgpt",
    plans: ["ChatGPT Go", "ChatGPT Plus", "ChatGPT Pro", "ChatGPT Business"],
  },
  { id: "claude", plans: ["Claude Max", "Claude Ultra", "Claude Team"] },
  { id: "google", plans: ["Google AI Pro", "Google AI Ultra"] },
  { id: "huggingface", plans: ["Hugging Face PRO"] },
  { id: "ollama", plans: ["Ollama Pro", "Ollama Max"] },
  { id: "none", plans: [] },
];
const SUBSCRIPTIONS: readonly Subscription[] = SUBSCRIPTION_OPTIONS.map(
  (option) => option.id,
);

/** The subscriptions whose way is a key to paste, with the provider it sets up. */
const GUIDED_KEY: Record<
  Exclude<Subscription, "chatgpt" | "none">,
  GuidedKeyProvider
> = {
  claude: "anthropic",
  google: "gemini",
  // A token with "Make calls to Inference Providers"; signing in comes later.
  huggingface: "huggingface",
  ollama: "ollama_cloud",
};

interface Answered {
  texts_may_go: TextsMayGo | null;
  subscription: Subscription | null;
  next_step: NextStep | null;
}

function oneOf<T extends string>(
  value: unknown,
  allowed: readonly T[],
): T | null {
  return typeof value === "string" &&
    (allowed as readonly string[]).includes(value)
    ? (value as T)
    : null;
}

function localModelFits(m: Machine): boolean {
  return m.memoryBytes >= LOCAL_MODEL_MIN_BYTES;
}

function clean(a: SetupAnswers, m?: Machine): Answered {
  const textsMayGo = oneOf(a.texts_may_go, TEXTS_MAY_GO);
  const subscription =
    textsMayGo === "free_services" || textsMayGo === "not_kept"
      ? oneOf(a.subscription, SUBSCRIPTIONS)
      : null;
  let nextStep =
    textsMayGo === "not_kept" && subscription === "none"
      ? oneOf(a.next_step, NEXT_STEPS)
      : null;
  // Greyed out on this computer, so it can't stay chosen.
  if (nextStep === "this_computer" && m !== undefined && !localModelFits(m)) {
    nextStep = null;
  }
  return { texts_may_go: textsMayGo, subscription, next_step: nextStep };
}

/**
 * Drops the answers that no longer apply after an earlier one changed, and
 * unknown values. Every field comes back, `null` where unanswered. With `m`,
 * "this computer" is dropped where the local model doesn't fit.
 */
export function sanitize(a: SetupAnswers, m?: Machine): SetupAnswers {
  return clean(a, m);
}

/** The question to ask next, or `null` once the answers lead to a way. */
export function nextQuestion(a: SetupAnswers, m?: Machine): 1 | 2 | 3 | null {
  const s = clean(a, m);
  if (s.texts_may_go === null) return 1;
  if (s.texts_may_go === "nowhere") return null;
  if (s.subscription === null) return 2;
  if (
    s.texts_may_go === "not_kept" &&
    s.subscription === "none" &&
    s.next_step === null
  ) {
    return 3;
  }
  return null;
}

/** The way the answers lead to on this computer, or `null` while a question is open. */
export function suggest(a: SetupAnswers, m: Machine): Way | null {
  const s = clean(a, m);
  const localModel: Way = { kind: "local_model", available: LOCAL_MODEL_BUILT };
  if (s.texts_may_go === "nowhere") {
    return localModelFits(m) ? localModel : { kind: "nowhere_unavailable" };
  }
  if (s.texts_may_go === null || s.subscription === null) return null;
  if (s.subscription === "chatgpt") {
    return {
      kind: "chatgpt_plan",
      dataNotDescribed: s.texts_may_go === "not_kept",
    };
  }
  if (s.subscription !== "none") {
    return { kind: "guided_key", provider: GUIDED_KEY[s.subscription] };
  }
  if (s.texts_may_go === "free_services") return { kind: "openrouter_free" };
  switch (s.next_step) {
    case "credit":
      return { kind: "openrouter_credit" };
    case "groq":
      return { kind: "guided_key", provider: "groq" };
    case "this_computer":
      return localModel;
    default:
      return null;
  }
}

/** The choices of question 2: alphabetically by provider, "none" last. */
export function subscriptionOptions(): { id: Subscription; plans: string[] }[] {
  return SUBSCRIPTION_OPTIONS.map((option) => ({
    id: option.id,
    plans: [...option.plans],
  }));
}

/** The choices of question 3; "this computer" last, greyed out below 16 GiB. */
export function nextStepOptions(
  m: Machine,
): { id: NextStep; disabled: boolean; reason?: "memory" }[] {
  return [
    { id: "credit", disabled: false },
    { id: "groq", disabled: false },
    localModelFits(m)
      ? { id: "this_computer", disabled: false }
      : { id: "this_computer", disabled: true, reason: "memory" },
  ];
}
