/**
 * DanyWhere: where each value of a profile is set and what it is called
 * (qa 2026-11-19_1711 P5, P8): its page among the defaults' pages, which a
 * profile has too, and the translation key of its name. The counts in the
 * navigation, the page "Profile", "Page to Defaults" and the confirmations
 * read this, so a value is placed once.
 *
 * Every field of `ProfileValues` needs a row: a field added to the backend
 * (and so to `bindings.ts`) breaks the type check here until it has one.
 */
import type { ProfileValues } from "@/bindings";
import type { DefaultsPage } from "./navigation";
import type { OwnValue } from "./profileScope";
import { soundThemeName } from "./soundThemeNames";

export type ProfileFieldName = keyof ProfileValues;

/** Where the labels of a setting's choices are, when the generic rule
 * ("…title" → "…options.<value in camelCase>") doesn't find them. */
export interface ProfileFieldOptions {
  /** The translation key the labels are under. */
  prefix: string;
  /** Stored values whose label isn't named after the value. */
  keys?: Readonly<Record<string, string>>;
  /** The label an empty value shows (the model's own effort, all
   * channels averaged). */
  empty?: string;
}

export interface ProfileFieldInfo {
  /** The page the setting is on. */
  page: DefaultsPage;
  /** Translation key of the setting's name, without placeholders. */
  label: string;
  /** The labels of its choices, for "Default: …" and the page "Profile". */
  options?: ProfileFieldOptions;
  /** A large setting (P7.2): under the defaults, a deviating profile shows
   * a summary with "Open in …" instead of its own control. */
  large?: boolean;
  /** Entries on another page than the field's, by entry id. */
  entryPages?: Readonly<Record<string, DefaultsPage>>;
  /** A string kept behind a switch: the stored value that turns it on;
   * any other shows as off, as the switch does (qa 2026-11-22_1731). */
  switchOn?: string;
  /** The name a choice's control shows, by stored value, where it isn't
   * found under `options` (the sound themes). */
  names?: (value: string, t: (key: string) => string) => string;
}

const FIELDS = "settings.profileView.fields";

export const PROFILE_FIELDS = {
  // General: shortcuts, end shortcuts, languages, the model's settings,
  // sound.
  bindings: {
    page: "general",
    label: `${FIELDS}.shortcuts`,
  },
  shortcut_activation: {
    page: "general",
    label: "settings.general.shortcutActivation.title",
  },
  end_keys: { page: "general", label: "settings.endKeys.title", large: true },
  // Under the defaults a deviating profile shows its ends for holding as
  // text ("⌃ Space held: …"), as the old default ends did.
  hold_ends: { page: "general", label: `${FIELDS}.holdEnds`, large: true },
  spoken_languages: {
    page: "general",
    label: "settings.general.preferredLanguages.title",
  },
  chinese_script: {
    page: "general",
    label: "settings.advanced.chineseScript.title",
  },
  selected_language: {
    page: "general",
    label: "settings.general.language.title",
  },
  translate_to_english: {
    page: "general",
    label: "settings.advanced.translateToEnglish.label",
  },
  selected_microphone: {
    page: "general",
    label: "settings.sound.microphone.title",
  },
  selected_channel: {
    page: "general",
    label: "settings.sound.channel.title",
    options: { prefix: "settings.sound.channel", empty: "average" },
  },
  mute_while_recording: {
    page: "general",
    label: "settings.debug.muteWhileRecording.label",
  },
  audio_feedback: {
    page: "general",
    label: "settings.sound.audioFeedback.label",
  },
  done_sound: { page: "general", label: "settings.sound.doneSound.label" },
  selected_output_device: {
    page: "general",
    label: "settings.sound.outputDevice.title",
  },
  audio_feedback_volume: {
    page: "general",
    label: "settings.sound.volume.title",
  },

  // Models: the speech model, a card of the table.
  selected_model: {
    page: "models",
    label: `${FIELDS}.selectedModel`,
    large: true,
  },

  // Post-processing.
  post_process_provider_id: {
    page: "postprocessing",
    label: "settings.postProcessing.api.provider.title",
  },
  post_process_providers: {
    page: "postprocessing",
    label: `${FIELDS}.providers`,
  },
  post_process_api_keys: {
    page: "postprocessing",
    label: "settings.postProcessing.api.apiKey.title",
  },
  post_process_models: {
    page: "postprocessing",
    label: "settings.postProcessing.api.model.title",
  },
  post_process_efforts: {
    page: "postprocessing",
    label: "settings.postProcessing.api.effort.title",
    options: {
      prefix: "settings.postProcessing.api.effort.levels",
      empty: "default",
    },
  },
  post_process_prompt_order: {
    page: "postprocessing",
    label: `${FIELDS}.activePrompts`,
    large: true,
  },
  context_age_limit_minutes: {
    page: "postprocessing",
    label: "settings.postProcessing.context.ageLimit",
  },
  context_max_entries: {
    page: "postprocessing",
    label: "settings.postProcessing.context.maxEntries",
  },
  context_char_budget: {
    page: "postprocessing",
    label: "settings.postProcessing.context.charBudget",
  },
  context_excluded_apps: {
    page: "postprocessing",
    label: "settings.postProcessing.context.excludedApps",
  },

  // Advanced: output, transcription, word lists, experimental.
  overlay_style: {
    page: "advanced",
    label: "settings.advanced.overlay.style.title",
  },
  overlay_position: {
    page: "advanced",
    label: "settings.advanced.overlay.position.title",
  },
  paste_method: {
    page: "advanced",
    label: "settings.advanced.pasteMethod.title",
    options: {
      prefix: "settings.advanced.pasteMethod.options",
      keys: {
        ctrl_v: "clipboard",
        ctrl_shift_v: "clipboardCtrlShiftV",
        shift_insert: "clipboardShiftInsert",
      },
    },
  },
  external_script_path: {
    page: "advanced",
    label: `${FIELDS}.externalScript`,
  },
  typing_tool: {
    page: "advanced",
    label: "settings.advanced.typingTool.title",
  },
  clipboard_handling: {
    page: "advanced",
    label: "settings.advanced.clipboardHandling.title",
  },
  clipboard_notice_ms: {
    page: "advanced",
    label: "settings.advanced.clipboardNotice.title",
  },
  vad_enabled: {
    page: "advanced",
    label: "settings.advanced.voiceActivityDetection.title",
  },
  long_form_chunking: {
    page: "advanced",
    label: "settings.advanced.longFormChunking.title",
  },
  filler_word_removal_enabled: {
    page: "advanced",
    label: "settings.advanced.fillerWordRemoval.title",
  },
  custom_filler_words: {
    page: "advanced",
    label: `${FIELDS}.customFillerWords`,
  },
  post_process_enabled: {
    page: "advanced",
    label: "settings.debug.postProcessingToggle.label",
  },
  post_process_timeout_secs: {
    page: "advanced",
    label: "settings.advanced.llmTimeouts.postProcessTitle",
  },
  custom_words: { page: "advanced", label: `${FIELDS}.customWords` },
  profile_words: { page: "advanced", label: `${FIELDS}.profileWords` },
  active_word_list_id: {
    page: "advanced",
    label: "settings.advanced.wordLists.activeTitle",
  },
  word_lists: { page: "advanced", label: `${FIELDS}.wordLists`, large: true },
  word_list_llm_check: {
    page: "advanced",
    label: "settings.advanced.wordLists.llmCheckTitle",
  },
  language_detection: {
    page: "advanced",
    label: "settings.advanced.languageDetection.title",
    // LanguageDetection.tsx: on is "always", off "when_needed".
    switchOn: "always",
  },
  language_detection_window_secs: {
    page: "advanced",
    label: "settings.debug.languageDetectionWindow.title",
  },
  sound_theme: {
    page: "advanced",
    label: "settings.debug.soundTheme.label",
    names: soundThemeName,
  },
  always_on_microphone: {
    page: "advanced",
    label: "settings.debug.alwaysOnMicrophone.label",
  },
  clamshell_microphone: {
    page: "advanced",
    label: "settings.debug.clamshellMicrophone.title",
  },
  airpods_mode: {
    page: "advanced",
    label: "settings.advanced.airpodsMode.label",
  },
} as const satisfies Record<ProfileFieldName, ProfileFieldInfo>;

const INFOS: Readonly<Record<string, ProfileFieldInfo>> = PROFILE_FIELDS;

/** The row of `field`, or `null` for a name that is no profile value. */
export const fieldInfo = (field: string): ProfileFieldInfo | null =>
  Object.prototype.hasOwnProperty.call(INFOS, field) ? INFOS[field] : null;

/** The names of a profile's values. */
export const PROFILE_FIELD_NAMES: ReadonlySet<string> = new Set(
  Object.keys(INFOS),
);

/** The page a field (or one of its entries) is on; `general` for a name
 * that is no profile value, which never happens for stored values. */
export const pageOfValue = (field: string, entry?: string): DefaultsPage => {
  const info = fieldInfo(field);
  if (!info) return "general";
  if (entry !== undefined && info.entryPages?.[entry]) {
    return info.entryPages[entry];
  }
  return info.page;
};

/** The fields whose page is `page`, entries placed elsewhere aside. */
export const fieldsOnPage = (page: DefaultsPage): string[] =>
  Object.entries(INFOS)
    .filter(([, info]) => info.page === page)
    .map(([field]) => field);

/**
 * What "Page to Defaults" couples again (P8): of a profile's own values
 * (`own`), those on `page`. A field whose entries all live on its page is
 * coupled as a whole; entries placed on another page than their field (the
 * post-processing shortcut) one by one, so the other page keeps its own.
 * `count` is how many own values that is, for the question before.
 */
export const resetPlan = (
  own: readonly OwnValue[],
  page: DefaultsPage,
): {
  fields: string[];
  entries: { field: string; entry: string }[];
  count: number;
} => {
  const fields: string[] = [];
  const entries: { field: string; entry: string }[] = [];
  let count = 0;
  for (const value of own) {
    if (pageOfValue(value.field, value.entry) !== page) continue;
    count += 1;
    const placedApart = fieldInfo(value.field)?.entryPages !== undefined;
    if (value.entry !== undefined && placedApart) {
      entries.push({ field: value.field, entry: value.entry });
    } else if (!fields.includes(value.field)) {
      fields.push(value.field);
    }
  }
  return { fields, entries, count };
};
