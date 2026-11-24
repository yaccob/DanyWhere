// The manual's screenshots: what the mocked backend answers. A fresh
// installation on macOS, in the language of the shot, with nothing of a
// real user's: no models, microphones, word lists, History or keys. Only
// made-up or factory values.
import type {
  AppendChoice,
  AppendComposed,
  AppendKey,
  AppendPart,
  AppSettings,
  EndKey,
} from "../../../src/bindings";
import { settingsWith } from "../../../src/test/fixtures";
import { answerCommand, ok } from "./mocks/bindings";

export type Theme = "light" | "dark";

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
  | "pattern"
  | "direct"
  | "postProcessed"
  | "space"
  | "noSpace"
  | "paragraph"
  | "line"
  | "enter"
  | "shiftEnter",
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

/** macOS's twelve factory end shortcuts, named in `language`: the list of
 * `factory_end_keys_on` in `src-tauri/src/end_keys.rs` (user, 2026-11-24).
 * Keep the two in step. */
export const factoryEndKeys = (language: string): EndKey[] => {
  const names = factoryNames(language);
  const name = (postProcess: boolean, effect: string) =>
    names.pattern
      .replace("{{mode}}", postProcess ? names.postProcessed : names.direct)
      .replace("{{effect}}", effect);
  // (id, combination, Append, post-processing, what it does)
  const ends: [string, string, string, boolean, string][] = [
    ["direct_space", "ctrl+space", " ", false, names.space],
    ["direct_no_space", "ctrl+shift+space", "", false, names.noSpace],
    ["direct_paragraph", "return", "\\n\\n", false, names.paragraph],
    ["direct_line", "shift+return", "\\n", false, names.line],
    ["direct_enter", "ctrl+return", "{Enter}", false, names.enter],
    [
      "direct_shift_enter",
      "ctrl+shift+return",
      "{Shift+Enter}",
      false,
      names.shiftEnter,
    ],
    ["post_space", "ctrl+option+space", " ", true, names.space],
    ["post_no_space", "ctrl+option+shift+space", "", true, names.noSpace],
    ["post_paragraph", "option+return", "\\n\\n", true, names.paragraph],
    ["post_line", "option+shift+return", "\\n", true, names.line],
    ["post_enter", "ctrl+option+return", "{Enter}", true, names.enter],
    [
      "post_shift_enter",
      "ctrl+option+shift+return",
      "{Shift+Enter}",
      true,
      names.shiftEnter,
    ],
  ];
  return ends.map(([id, shortcut, append_text, post_process, effect]) => ({
    id,
    name: name(post_process, effect),
    shortcut,
    append_text,
    post_process,
    // "Send Context" exactly where it post-processes.
    send_context: post_process,
  }));
};

/** The settings of a fresh macOS installation in `language`, onboarding
 * done, with the app theme `theme`. */
export const freshMacSettings = (language: string, theme: Theme): AppSettings =>
  settingsWith({
    defaults: {
      shortcut_activation: "hold_or_toggle",
      end_keys: factoryEndKeys(language),
      hold_ends: {
        transcribe: {
          append_text: " ",
          post_process: false,
          send_context: false,
        },
        transcribe_with_post_process: {
          append_text: " ",
          post_process: true,
          send_context: true,
        },
      },
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
  end_keys: { kind: "list", key: "id" },
} as const;

/** The mocked backend answers as a fresh macOS installation in `language`. */
export const answerAsFreshMac = (language: string, theme: Theme): void => {
  const settings = freshMacSettings(language, theme);
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
};
