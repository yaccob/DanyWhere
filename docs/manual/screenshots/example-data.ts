// The manual's screenshots: what the mocked backend answers. A fresh
// installation on macOS, in the language of the shot, with post-processing
// set up with a made-up provider, and nothing of a real user's: no models,
// microphones, word lists, History or real keys. Only made-up or factory
// values. The scene "profiles" adds two example profiles (`EXAMPLE_PROFILES`).
import type {
  AppendChoice,
  AppendComposed,
  AppendKey,
  AppendPart,
  ApplicationInfo,
  AppSettings,
  EndKey,
  StartKey,
} from "../../../src/bindings";
import { settingsWith, type ProfileSpec } from "../../../src/test/fixtures";
import { answerCommand, ok } from "./mocks/bindings";

export type Theme = "light" | "dark";

/** What the window holds: a fresh installation, or the same with the two
 * example profiles (`app.html?scene=profiles`). */
export type Scene = "fresh" | "profiles";

/** Applications a Mac might list for assigning to a profile
 * (`list_applications`), running ones first as the backend sorts them. */
export const EXAMPLE_APPLICATIONS: ApplicationInfo[] = [
  { id: "com.googlecode.iterm2", name: "iTerm2", running: true },
  { id: "com.tinyspeck.slackmacgap", name: "Slack", running: true },
  { id: "com.apple.mail", name: "Mail", running: true },
  { id: "com.apple.Terminal", name: "Terminal", running: false },
  { id: "org.whispersystems.signal-desktop", name: "Signal", running: false },
  { id: "com.apple.TextEdit", name: "TextEdit", running: false },
];

/** macOS's factory start shortcut: ⌃ Space (`factory_start_keys_on` in
 * `src-tauri/src/start_keys.rs`). */
const factoryStartKeys = (): StartKey[] => [
  { id: "start", shortcut: "ctrl+space" },
];

/** The id of the factory hold shortcut ⌃ Space (`HOLD_SPACE_ID` in
 * `src-tauri/src/start_keys.rs`). */
const HOLD_SPACE_ID = "hold_space";

/** macOS's factory hold shortcuts, unnamed, so they show "⌃ Space held":
 * ⌃ Space appends a space, ⌃⇧ Space nothing, both with "Send Context"
 * (`factory_hold_keys_on` in `src-tauri/src/start_keys.rs`). */
const factoryHoldKeys = (): EndKey[] => [
  {
    id: HOLD_SPACE_ID,
    name: "",
    shortcut: "ctrl+space",
    append_text: " ",
    send_context: true,
  },
  {
    id: "hold_no_space",
    name: "",
    shortcut: "ctrl+shift+space",
    append_text: "",
    send_context: true,
  },
];

/** macOS's factory instruction shortcut ⇥, unnamed ("⇥ held"), appending a
 * space, with "Send Context" (`factory_instruction_keys_on` in
 * `src-tauri/src/instruction_keys.rs`). */
const factoryInstructionKeys = (): EndKey[] => [
  {
    id: "instruction_tab",
    name: "",
    shortcut: "tab",
    append_text: " ",
    send_context: true,
  },
];

/** Two profiles, each with one value of its own; everything else follows
 * the defaults:
 *
 * - "Terminal" (iTerm2, Terminal) never post-processes ("Post Processing":
 *   "Never"), so a command is typed as spoken and ⇥ stays Tab;
 * - "Chat" (Slack, Signal) appends Enter when the hold shortcut ⌃ Space is
 *   released, so the message goes out: its own version of that one entry.
 *
 * Choosing the profile by application is on; the defaults are in use. */
export const EXAMPLE_PROFILES: ProfileSpec[] = [
  {
    id: "profile_1795300000001",
    name: "Terminal",
    apps: ["com.googlecode.iterm2", "com.apple.Terminal"],
    values: { post_process_mode: "never" },
    own: ["post_process_mode"],
  },
  {
    id: "profile_1795300000002",
    name: "Chat",
    apps: ["com.tinyspeck.slackmacgap", "org.whispersystems.signal-desktop"],
    values: {
      hold_keys: factoryHoldKeys().map((hold) =>
        hold.id === HOLD_SPACE_ID ? { ...hold, append_text: "{Enter}" } : hold,
      ),
    },
    ownEntries: { hold_keys: { own: [HOLD_SPACE_ID] } },
  },
];

/** The locale files, for the factory end shortcuts' names. */
const LOCALES = import.meta.glob<{ default: Record<string, unknown> }>(
  "../../../src/i18n/locales/*/translation.json",
  { eager: true },
);

const localeOf = (language: string): Record<string, unknown> => {
  const entry = Object.entries(LOCALES).find(([path]) =>
    path.endsWith(`/locales/${language}/translation.json`),
  );
  if (!entry) throw new Error(`no locale file for ${language}`);
  return entry[1].default;
};

type FactoryNames = Record<
  "space" | "noSpace" | "paragraph" | "line" | "enter" | "shiftEnter",
  string
>;

/** `settings.endKeys.factoryNames` of a locale, English where it lacks one,
 * as build.rs gives the tray (`FactoryEndNames::for_locale`). */
const factoryNames = (language: string): FactoryNames => {
  const of = (tree: Record<string, unknown>) =>
    ((tree.settings as Record<string, Record<string, unknown>> | undefined)
      ?.endKeys?.factoryNames ?? {}) as Partial<FactoryNames>;
  return { ...(of(localeOf("en")) as FactoryNames), ...of(localeOf(language)) };
};

/** macOS's six factory end shortcuts, named in `language` by what they do,
 * all with "Send Context": `MACOS_SIX` and `factory_end_keys_on` in
 * `src-tauri/src/end_keys.rs` (user, 2026-11-26, variant b). Keep the two in
 * step. */
export const factoryEndKeys = (language: string): EndKey[] => {
  const names = factoryNames(language);
  // (id, combination, Append, name)
  const ends: [string, string, string, string][] = [
    ["direct_space", "ctrl+space", " ", names.space],
    ["direct_no_space", "ctrl+shift+space", "", names.noSpace],
    ["direct_paragraph", "return", "\\n\\n", names.paragraph],
    ["direct_line", "shift+return", "\\n", names.line],
    ["direct_enter", "ctrl+return", "{Enter}", names.enter],
    ["direct_shift_enter", "ctrl+shift+return", "{Shift+Enter}", names.shiftEnter],
  ];
  return ends.map(([id, shortcut, append_text, name]) => ({
    id,
    name,
    shortcut,
    append_text,
    send_context: true,
  }));
};

/** Post-processing set up with a made-up provider and model, so the pages
 * show the instruction shortcuts as taken (the manual explains them). */
const EXAMPLE_PROVIDER = {
  id: "custom",
  label: "Custom",
  base_url: "https://llm.example.com/v1",
  allow_base_url_edit: true,
  models_endpoint: "/models",
  supports_structured_output: false,
  api_format: "openai",
} as const;

/** The settings of a fresh macOS installation in `language`, onboarding
 * done, with the app theme `theme` and post-processing set up
 * (`EXAMPLE_PROVIDER`); in the scene "profiles" with `EXAMPLE_PROFILES`. */
export const freshMacSettings = (
  language: string,
  theme: Theme,
  scene: Scene = "fresh",
): AppSettings =>
  settingsWith({
    ...(scene === "profiles" ? { profiles: EXAMPLE_PROFILES } : {}),
    defaults: {
      post_process_mode: "with_instruction_key",
      post_process_provider_id: EXAMPLE_PROVIDER.id,
      post_process_providers: [EXAMPLE_PROVIDER],
      post_process_models: { [EXAMPLE_PROVIDER.id]: "example-model" },
      post_process_api_keys: { [EXAMPLE_PROVIDER.id]: "example-key" },
      start_keys: factoryStartKeys(),
      hold_keys: factoryHoldKeys(),
      instruction_keys: factoryInstructionKeys(),
      end_keys: factoryEndKeys(language),
    },
    app: {
      app_language: language,
      theme,
      keyboard_implementation: "handy_keys",
      show_whats_new_on_update: false,
      update_checks_enabled: false,
    } as Partial<AppSettings>,
  });

// ---- the "Append" field (src-tauri/src/append_text.rs, macOS) ---------------

const KEY_NAMES: Record<AppendKey, string> = {
  enter: "Enter",
  tab: "Tab",
  shift_enter: "Shift+Enter",
  cmd_enter: "Cmd+Enter",
  ctrl_enter: "Ctrl+Enter",
};
const KEY_OF_NAME = Object.fromEntries(
  Object.entries(KEY_NAMES).map(([key, name]) => [name.toLowerCase(), key]),
) as Record<string, AppendKey>;

/** `split` for the values the factory ends have: text with `\s`, `\n`,
 * `\t` escapes, then key presses in braces. */
const splitAppend = (value: string): AppendChoice => {
  const keys: AppendKey[] = [];
  let text = value;
  for (;;) {
    const match = /\{([^{}]+)\}$/.exec(text);
    const key = match && KEY_OF_NAME[match[1].toLowerCase()];
    if (!match || !key) break;
    keys.unshift(key);
    text = text.slice(0, match.index);
  }
  return { text, keys, kept: keys.length > 1 };
};

/** `parse`: the parts the preview under the field names. */
const partsOf = (text: string, keys: AppendKey[]): AppendPart[] => {
  const parts: AppendPart[] = [];
  let run = "";
  const flush = () => {
    // Only spaces count, as in `push_text`.
    const inner = run.replace(/^ +| +$/g, "");
    const leading = run.length - run.replace(/^ +/, "").length;
    const trailing = inner ? run.length - run.replace(/ +$/, "").length : 0;
    for (let i = 0; i < leading; i += 1) parts.push({ kind: "space" });
    if (inner) parts.push({ kind: "text", text: inner });
    for (let i = 0; i < trailing; i += 1) parts.push({ kind: "space" });
    run = "";
  };
  for (let i = 0; i < text.length; i += 1) {
    const escaped = text[i] === "\\" ? text[i + 1] : undefined;
    const part: AppendPart | null =
      escaped === "s"
        ? { kind: "space" }
        : escaped === "n"
          ? { kind: "new_line" }
          : escaped === "t"
            ? { kind: "tab" }
            : null;
    if (part) {
      flush();
      parts.push(part);
      i += 1;
    } else {
      run += text[i];
    }
  }
  flush();
  return [...parts, ...keys.map((key) => ({ kind: "key" as const, key }))];
};

const composeAppend = (text: string, keys: AppendKey[]): AppendComposed => ({
  value: text + keys.map((key) => `{${KEY_NAMES[key]}}`).join(""),
  parts: partsOf(text, keys),
});

/** What the profile layers call list or map fields
 * (`profile_layers::LIST_FIELDS`, as `src/test/bindings-mock.ts` has them). */
const LIST_FIELDS = {
  word_lists: { kind: "list", key: "id" },
  post_process_providers: { kind: "list", key: "id" },
  post_process_models: { kind: "map" },
  post_process_efforts: { kind: "map" },
  post_process_api_keys: { kind: "map" },
  bindings: { kind: "map" },
  start_keys: { kind: "list", key: "id" },
  hold_keys: { kind: "list", key: "id" },
  instruction_keys: { kind: "list", key: "id" },
  end_keys: { kind: "list", key: "id" },
} as const;

/** The mocked backend answers as a fresh macOS installation in `language`,
 * in the scene "profiles" with the example profiles. */
export const answerAsFreshMac = (
  language: string,
  theme: Theme,
  scene: Scene = "fresh",
): void => {
  const settings = freshMacSettings(language, theme, scene);
  answerCommand("getAppSettings", async () => ok(settings));
  answerCommand("getDefaultSettings", async () => ok(settings));
  answerCommand("getProfileFields", async () =>
    Object.entries(LIST_FIELDS).map(([name, kind]) => ({ name, kind })),
  );
  answerCommand("appendKeyOptions", async () => [
    "enter",
    "shift_enter",
    "cmd_enter",
    "ctrl_enter",
    "tab",
  ]);
  answerCommand("splitAppendText", async (value) => splitAppend(value));
  answerCommand("composeAppendText", async (text, keys) =>
    composeAppend(text, keys),
  );
  answerCommand("checkCustomSounds", async () => ({
    start: false,
    stop: false,
  }));
  if (scene === "profiles") {
    answerCommand("listApplications", async () => EXAMPLE_APPLICATIONS);
  }
};
