// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/postProcessSetup/questions.test.ts
import assert from "node:assert";
import {
  LOCAL_MODEL_MIN_BYTES,
  nextQuestion,
  nextStepOptions,
  sanitize,
  subscriptionOptions,
  suggest,
  type GuidedKeyProvider,
  type Machine,
  type NextStep,
  type SetupAnswers,
  type Subscription,
  type TextsMayGo,
  type Way,
} from "./questions";

type Case = [string, () => void];

const GIB = 1024 ** 3;
const machine = (
  memoryBytes: number,
  platform: Machine["platform"] = "macos",
): Machine => ({ memoryBytes, platform });
const MAC_16 = machine(16 * GIB);
const MAC_8 = machine(8 * GIB);
const JUST_UNDER_15 = machine(15 * GIB - 1);
const LINUX_32 = machine(32 * GIB, "linux");
const WINDOWS_8 = machine(8 * GIB, "windows");
// 16 GB as Linux (`MemTotal`) and Windows (`GlobalMemoryStatusEx`) report it.
const LINUX_16 = machine(Math.round(15.3 * GIB), "linux");
const WINDOWS_16 = machine(Math.round(15.8 * GIB), "windows");
const MACHINES = [
  MAC_8,
  MAC_16,
  JUST_UNDER_15,
  LINUX_32,
  WINDOWS_8,
  LINUX_16,
  WINDOWS_16,
];
const describe = (m: Machine) =>
  `${m.platform} with ${(m.memoryBytes / GIB).toFixed(2)} GiB`;

const NONE_ANSWERED = {
  texts_may_go: null,
  subscription: null,
  next_step: null,
} as const;
const answers = (a: SetupAnswers): SetupAnswers => ({ ...NONE_ANSWERED, ...a });

// Question 1's two answers that lead on to question 2.
const CLOUD: TextsMayGo[] = ["free_services", "not_kept"];
// Subscriptions whose way is a guided key, with the provider it sets up.
// Hugging Face starts as a guided key (a token with "Make calls to Inference
// Providers"); signing in with OAuth comes later.
const GUIDED: Array<[Subscription, GuidedKeyProvider]> = [
  ["claude", "anthropic"],
  ["google", "gemini"],
  ["huggingface", "huggingface"],
  ["ollama", "ollama_cloud"],
];
const SUBSCRIPTIONS: Subscription[] = [
  "chatgpt",
  "claude",
  "google",
  "huggingface",
  "ollama",
];

// The table "where the answers lead": question 1, question 2, question 3,
// the suggested way. Cloud ways don't depend on the machine, so those rows
// are checked on every machine; the local model's rows name theirs.
const CLOUD_ROWS: Array<[string, SetupAnswers, Way]> = [
  [
    "free services, no subscription: OpenRouter free",
    { texts_may_go: "free_services", subscription: "none" },
    { kind: "openrouter_free" },
  ],
  [
    "free services, ChatGPT: the plan, its data handling not in question",
    { texts_may_go: "free_services", subscription: "chatgpt" },
    { kind: "chatgpt_plan", dataNotDescribed: false },
  ],
  [
    "not kept, ChatGPT: the plan, with the note that OpenAI doesn't describe the handling",
    { texts_may_go: "not_kept", subscription: "chatgpt" },
    { kind: "chatgpt_plan", dataNotDescribed: true },
  ],
  ...CLOUD.flatMap((texts) =>
    GUIDED.map(([subscription, provider]): [string, SetupAnswers, Way] => [
      `${texts}, ${subscription}: a guided key for ${provider}`,
      { texts_may_go: texts, subscription },
      { kind: "guided_key", provider },
    ]),
  ),
  [
    "not kept, no subscription, 5 $: OpenRouter with credit",
    { texts_may_go: "not_kept", subscription: "none", next_step: "credit" },
    { kind: "openrouter_credit" },
  ],
  [
    "not kept, no subscription, Groq: a guided key for Groq",
    { texts_may_go: "not_kept", subscription: "none", next_step: "groq" },
    { kind: "guided_key", provider: "groq" },
  ],
];

// Until stage 2 the local model isn't built: the way says so ("coming in a
// later version" and a server of one's own), from 16 GiB only.
const LOCAL_ROWS: Array<[string, SetupAnswers, Machine, Way | null]> = [
  [
    "not kept, no subscription, this computer, 16 GiB: the local model (not built yet)",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "this_computer",
    },
    MAC_16,
    { kind: "local_model", available: false },
  ],
  [
    "not kept, no subscription, this computer, 32 GiB: the local model (not built yet)",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "this_computer",
    },
    LINUX_32,
    { kind: "local_model", available: false },
  ],
  [
    // Greyed out below 16 GiB, so a stored choice of it counts as unanswered.
    "not kept, no subscription, this computer, 8 GiB: no suggestion, the option is greyed out",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "this_computer",
    },
    MAC_8,
    null,
  ],
  [
    "not kept, no subscription, this computer, 1 byte under 15 GiB: no suggestion",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "this_computer",
    },
    JUST_UNDER_15,
    null,
  ],
  [
    "nowhere, 16 GiB: the local model (not built yet)",
    { texts_may_go: "nowhere" },
    MAC_16,
    { kind: "local_model", available: false },
  ],
  [
    "nowhere, 32 GiB: the local model (not built yet)",
    { texts_may_go: "nowhere" },
    LINUX_32,
    { kind: "local_model", available: false },
  ],
  [
    "nowhere, 16 GB on Linux (15.3 GiB reported): the local model (not built yet)",
    { texts_may_go: "nowhere" },
    LINUX_16,
    { kind: "local_model", available: false },
  ],
  [
    "nowhere, 16 GB on Windows (15.8 GiB reported): the local model (not built yet)",
    { texts_may_go: "nowhere" },
    WINDOWS_16,
    { kind: "local_model", available: false },
  ],
  [
    "nowhere, 8 GiB: not possible on this computer",
    { texts_may_go: "nowhere" },
    MAC_8,
    { kind: "nowhere_unavailable" },
  ],
  [
    "nowhere, 8 GiB on Windows: not possible on this computer",
    { texts_may_go: "nowhere" },
    WINDOWS_8,
    { kind: "nowhere_unavailable" },
  ],
  [
    "nowhere, 1 byte under 15 GiB: not possible on this computer",
    { texts_may_go: "nowhere" },
    JUST_UNDER_15,
    { kind: "nowhere_unavailable" },
  ],
];

const tableCases: Case[] = [
  ...CLOUD_ROWS.flatMap(([name, given, way]) =>
    MACHINES.map(
      (m): Case => [
        `table: ${name} (${describe(m)})`,
        () => assert.deepStrictEqual(suggest(given, m), way),
      ],
    ),
  ),
  ...LOCAL_ROWS.map(
    ([name, given, m, way]): Case => [
      `table: ${name}`,
      () => assert.deepStrictEqual(suggest(given, m), way),
    ],
  ),
];

// While a question is still open there is nothing to suggest.
const PARTIAL: Array<[string, SetupAnswers]> = [
  ["nothing answered", {}],
  ["every answer null", NONE_ANSWERED],
  ["free services only", { texts_may_go: "free_services" }],
  ["not kept only", { texts_may_go: "not_kept" }],
  [
    "not kept, no subscription",
    { texts_may_go: "not_kept", subscription: "none" },
  ],
  ["a subscription without question 1", { subscription: "chatgpt" }],
  ["a next step without question 1", { next_step: "credit" }],
];

const partialCases: Case[] = PARTIAL.flatMap(([name, given]) =>
  [MAC_8, MAC_16].map(
    (m): Case => [
      `no suggestion while a question is open: ${name} (${describe(m)})`,
      () => assert.strictEqual(suggest(given, m), null),
    ],
  ),
);

const staleCases: Case[] = [
  [
    "nowhere wins over a subscription left from before",
    () =>
      assert.deepStrictEqual(
        suggest({ texts_may_go: "nowhere", subscription: "chatgpt" }, MAC_16),
        { kind: "local_model", available: false },
      ),
  ],
  [
    "nowhere below 16 GiB wins over a next step left from before",
    () =>
      assert.deepStrictEqual(
        suggest(
          {
            texts_may_go: "nowhere",
            subscription: "none",
            next_step: "credit",
          },
          MAC_8,
        ),
        { kind: "nowhere_unavailable" },
      ),
  ],
  [
    "free services ignore a next step left from before",
    () =>
      assert.deepStrictEqual(
        suggest(
          {
            texts_may_go: "free_services",
            subscription: "none",
            next_step: "credit",
          },
          MAC_16,
        ),
        { kind: "openrouter_free" },
      ),
  ],
  [
    "a subscription ignores a next step left from before",
    () =>
      assert.deepStrictEqual(
        suggest(
          {
            texts_may_go: "not_kept",
            subscription: "claude",
            next_step: "groq",
          },
          MAC_16,
        ),
        { kind: "guided_key", provider: "anthropic" },
      ),
  ],
  [
    "an unknown stored answer counts as unanswered",
    () =>
      assert.strictEqual(
        suggest(
          {
            texts_may_go: "free_services",
            subscription: "mistral" as Subscription,
          },
          MAC_16,
        ),
        null,
      ),
  ],
];

const NEXT: Array<[string, SetupAnswers, 1 | 2 | 3 | null]> = [
  ["nothing answered: question 1", {}, 1],
  ["every answer null: question 1", NONE_ANSWERED, 1],
  [
    "a subscription without question 1: question 1",
    { subscription: "none" },
    1,
  ],
  [
    "an unknown answer to question 1: question 1",
    { texts_may_go: "secret" as TextsMayGo, subscription: "none" },
    1,
  ],
  ["free services: question 2", { texts_may_go: "free_services" }, 2],
  ["not kept: question 2", { texts_may_go: "not_kept" }, 2],
  [
    "not kept with an unknown subscription: question 2",
    { texts_may_go: "not_kept", subscription: "grok" as Subscription },
    2,
  ],
  ["nowhere: no further question", { texts_may_go: "nowhere" }, null],
  [
    "nowhere with answers left from before: no further question",
    { texts_may_go: "nowhere", subscription: "none" },
    null,
  ],
  [
    "free services, no subscription: no further question",
    { texts_may_go: "free_services", subscription: "none" },
    null,
  ],
  ...CLOUD.flatMap((texts) =>
    SUBSCRIPTIONS.map((subscription): [string, SetupAnswers, null] => [
      `${texts}, ${subscription}: no further question`,
      { texts_may_go: texts, subscription },
      null,
    ]),
  ),
  [
    "not kept, no subscription: question 3",
    { texts_may_go: "not_kept", subscription: "none" },
    3,
  ],
  [
    "not kept, no subscription, an unknown next step: question 3",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "later" as NextStep,
    },
    3,
  ],
  ...(["credit", "groq", "this_computer"] as NextStep[]).map(
    (step): [string, SetupAnswers, null] => [
      `not kept, no subscription, ${step}: no further question`,
      { texts_may_go: "not_kept", subscription: "none", next_step: step },
      null,
    ],
  ),
];

const THIS_COMPUTER: SetupAnswers = {
  texts_may_go: "not_kept",
  subscription: "none",
  next_step: "this_computer",
};

const nextQuestionCases: Case[] = [
  ...NEXT.map(
    ([name, given, expected]): Case => [
      `next question, ${name}`,
      () => assert.strictEqual(nextQuestion(given), expected),
    ],
  ),
  [
    "next question, this computer below 16 GiB is greyed out: question 3 again",
    () => assert.strictEqual(nextQuestion(THIS_COMPUTER, MAC_8), 3),
  ],
  [
    "next question, this computer with 16 GiB: no further question",
    () => assert.strictEqual(nextQuestion(THIS_COMPUTER, MAC_16), null),
  ],
  [
    "next question, a machine changes nothing for other next steps",
    () =>
      assert.strictEqual(
        nextQuestion(
          { texts_may_go: "not_kept", subscription: "none", next_step: "groq" },
          MAC_8,
        ),
        null,
      ),
  ],
];

const SANITIZE: Array<[string, SetupAnswers, SetupAnswers]> = [
  ["nothing answered gives every answer as null", {}, NONE_ANSWERED],
  [
    "a full answer that still holds stays",
    THIS_COMPUTER,
    answers(THIS_COMPUTER),
  ],
  [
    "free services and a subscription stay",
    { texts_may_go: "free_services", subscription: "ollama" },
    answers({ texts_may_go: "free_services", subscription: "ollama" }),
  ],
  [
    "question 1 changed to nowhere drops the subscription and the next step",
    { texts_may_go: "nowhere", subscription: "none", next_step: "credit" },
    answers({ texts_may_go: "nowhere" }),
  ],
  [
    "question 1 changed to nowhere drops a subscription",
    { texts_may_go: "nowhere", subscription: "chatgpt" },
    answers({ texts_may_go: "nowhere" }),
  ],
  [
    "question 1 changed to free services drops the next step and keeps no subscription",
    { texts_may_go: "free_services", subscription: "none", next_step: "groq" },
    answers({ texts_may_go: "free_services", subscription: "none" }),
  ],
  [
    "question 1 changed from not kept to free services keeps a subscription",
    { texts_may_go: "free_services", subscription: "chatgpt" },
    answers({ texts_may_go: "free_services", subscription: "chatgpt" }),
  ],
  [
    "question 2 changed to a subscription drops the next step",
    { texts_may_go: "not_kept", subscription: "google", next_step: "credit" },
    answers({ texts_may_go: "not_kept", subscription: "google" }),
  ],
  [
    "question 1 cleared drops everything after it",
    { texts_may_go: null, subscription: "none", next_step: "credit" },
    NONE_ANSWERED,
  ],
  [
    "question 2 cleared drops the next step",
    { texts_may_go: "not_kept", subscription: null, next_step: "credit" },
    answers({ texts_may_go: "not_kept" }),
  ],
  [
    "an unknown answer to question 1 drops everything",
    { texts_may_go: "secret" as TextsMayGo, subscription: "none" },
    NONE_ANSWERED,
  ],
  [
    "an unknown subscription is dropped with what follows",
    {
      texts_may_go: "not_kept",
      subscription: "mistral" as Subscription,
      next_step: "credit",
    },
    answers({ texts_may_go: "not_kept" }),
  ],
  [
    "an unknown next step is dropped",
    {
      texts_may_go: "not_kept",
      subscription: "none",
      next_step: "later" as NextStep,
    },
    answers({ texts_may_go: "not_kept", subscription: "none" }),
  ],
];

const sanitizeCases: Case[] = [
  ...SANITIZE.map(
    ([name, given, expected]): Case => [
      `sanitize: ${name}`,
      () => assert.deepStrictEqual(sanitize(given), expected),
    ],
  ),
  ...SANITIZE.map(
    ([name, given]): Case => [
      `sanitize twice changes nothing more: ${name}`,
      () => assert.deepStrictEqual(sanitize(sanitize(given)), sanitize(given)),
    ],
  ),
  [
    "sanitize: this computer below 16 GiB is dropped, being greyed out",
    () =>
      assert.deepStrictEqual(
        sanitize(THIS_COMPUTER, MAC_8),
        answers({ texts_may_go: "not_kept", subscription: "none" }),
      ),
  ],
  [
    "sanitize: this computer with 16 GiB stays",
    () =>
      assert.deepStrictEqual(
        sanitize(THIS_COMPUTER, MAC_16),
        answers(THIS_COMPUTER),
      ),
  ],
  [
    "sanitize: without a machine this computer stays",
    () =>
      assert.deepStrictEqual(sanitize(THIS_COMPUTER), answers(THIS_COMPUTER)),
  ],
  [
    "sanitize: the answers given are left as they were",
    () => {
      const given: SetupAnswers = {
        texts_may_go: "nowhere",
        subscription: "none",
        next_step: "credit",
      };
      sanitize(given, MAC_8);
      assert.deepStrictEqual(given, {
        texts_may_go: "nowhere",
        subscription: "none",
        next_step: "credit",
      });
    },
  ],
];

const optionCases: Case[] = [
  [
    "subscriptions: alphabetical by provider, none last",
    () =>
      assert.deepStrictEqual(
        subscriptionOptions().map((option) => option.id),
        ["chatgpt", "claude", "google", "huggingface", "ollama", "none"],
      ),
  ],
  [
    "subscriptions: each with the plans that include use in other apps",
    () =>
      assert.deepStrictEqual(subscriptionOptions(), [
        {
          id: "chatgpt",
          plans: [
            "ChatGPT Go",
            "ChatGPT Plus",
            "ChatGPT Pro",
            "ChatGPT Business",
          ],
        },
        { id: "claude", plans: ["Claude Max", "Claude Ultra", "Claude Team"] },
        { id: "google", plans: ["Google AI Pro", "Google AI Ultra"] },
        { id: "huggingface", plans: ["Hugging Face PRO"] },
        { id: "ollama", plans: ["Ollama Pro", "Ollama Max"] },
        { id: "none", plans: [] },
      ]),
  ],
  [
    "subscriptions: the providers' product names are in alphabetical order",
    () => {
      const names = subscriptionOptions()
        .filter((option) => option.id !== "none")
        .map((option) => option.plans[0]);
      assert.deepStrictEqual(
        [...names].sort((x, y) => x.localeCompare(y, "en")),
        names,
      );
    },
  ],
  [
    "subscriptions: Claude Pro doesn't count",
    () =>
      assert.ok(
        !subscriptionOptions().some((option) =>
          option.plans.includes("Claude Pro"),
        ),
      ),
  ],
  [
    "subscriptions: changing the list given out changes nothing",
    () => {
      const first = subscriptionOptions();
      first[0].plans.push("ChatGPT Free");
      first.reverse();
      assert.deepStrictEqual(subscriptionOptions()[0], {
        id: "chatgpt",
        plans: [
          "ChatGPT Go",
          "ChatGPT Plus",
          "ChatGPT Pro",
          "ChatGPT Business",
        ],
      });
    },
  ],
  [
    "next steps with 16 GiB: credit, Groq, this computer last, none greyed out",
    () =>
      assert.deepStrictEqual(nextStepOptions(MAC_16), [
        { id: "credit", disabled: false },
        { id: "groq", disabled: false },
        { id: "this_computer", disabled: false },
      ]),
  ],
  [
    "next steps with 32 GiB on Linux: none greyed out",
    () =>
      assert.deepStrictEqual(nextStepOptions(LINUX_32), [
        { id: "credit", disabled: false },
        { id: "groq", disabled: false },
        { id: "this_computer", disabled: false },
      ]),
  ],
  [
    "next steps with 8 GiB: this computer greyed out for memory, still last",
    () =>
      assert.deepStrictEqual(nextStepOptions(MAC_8), [
        { id: "credit", disabled: false },
        { id: "groq", disabled: false },
        { id: "this_computer", disabled: true, reason: "memory" },
      ]),
  ],
  [
    "next steps 1 byte under 15 GiB: this computer greyed out for memory",
    () =>
      assert.deepStrictEqual(nextStepOptions(JUST_UNDER_15)[2], {
        id: "this_computer",
        disabled: true,
        reason: "memory",
      }),
  ],
  [
    "next steps on a 16 GB Linux machine (15.3 GiB reported): this computer can be chosen",
    () =>
      assert.deepStrictEqual(nextStepOptions(LINUX_16)[2], {
        id: "this_computer",
        disabled: false,
      }),
  ],
  [
    "the local model needs 15 GiB as reported (a computer with 16 GB)",
    () => assert.strictEqual(LOCAL_MODEL_MIN_BYTES, 16_106_127_360),
  ],
];

const cases: Case[] = [
  ...tableCases,
  ...partialCases,
  ...staleCases,
  ...nextQuestionCases,
  ...sanitizeCases,
  ...optionCases,
];

let failed = 0;
for (const [name, check] of cases) {
  try {
    check();
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${(error as Error).message.split("\n")[0]}`);
  }
}
if (failed > 0) {
  console.error(
    `postProcessSetup/questions: ${failed} of ${cases.length} checks failed (${cases.length - failed} passed)`,
  );
  process.exit(1);
}
console.log(`postProcessSetup/questions: all ${cases.length} checks passed`);
