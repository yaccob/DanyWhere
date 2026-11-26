// DanyWhere: settings for component tests, in the shape the backend sends
// them (`get_app_settings`): the values in use at the top level, the
// defaults in `default_values`, each profile with all its values and which
// of them are its own (`own`, `own_entries`). Only what the settings pages
// read is filled in; a test changes what it is about.
import type {
  AppSettings,
  EndKey,
  LLMPrompt,
  ModelInfo,
  Profile,
  ShortcutBinding,
  StartKey,
  WordList,
} from "@/bindings";

type Values = Record<string, unknown>;

export const binding = (
  id: string,
  keys: string,
  extra: Partial<ShortcutBinding> = {},
): ShortcutBinding => ({
  id,
  name: id,
  description: "",
  default_binding: keys,
  current_binding: keys,
  ...extra,
});

export const endKey = (
  id: string,
  shortcut: string,
  extra: Partial<EndKey> = {},
): EndKey => ({
  id,
  name: "",
  shortcut,
  append_text: "",
  post_process: false,
  send_context: false,
  ...extra,
});

/** A start shortcut: only its combination (qa 2026-11-26_1810). */
export const startKey = (id: string, shortcut: string): StartKey => ({
  id,
  shortcut,
});

/** A hold shortcut: the shape of an end shortcut (qa 2026-11-26_1810). */
export const holdKey = endKey;

export const wordList = (id: string, name: string): WordList => ({
  id,
  name,
  words: [],
});

export const prompt = (id: string, name: string): LLMPrompt => ({
  id,
  name,
  prompt: `${name} prompt`,
});

/** A speech model as `get_available_models` lists it: a catalog model on
 * disk, without vocabulary unless `extra` says so. */
export const modelInfo = (
  id: string,
  name: string,
  extra: Partial<ModelInfo> = {},
): ModelInfo => ({
  id,
  name,
  description: "",
  filename: `${id}.gguf`,
  source: { HuggingFace: { repo_id: `test/${id}`, revision: "main" } },
  size_mb: 1000,
  is_downloaded: true,
  is_downloading: false,
  partial_size: 0,
  accuracy_score: 0.8,
  speed_score: 0.8,
  supports_translation: false,
  is_recommended: false,
  supported_languages: ["en", "de"],
  supports_language_selection: true,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
  supports_vocabulary: false,
  ...extra,
});

/** The defaults' values as DanyWhere ships them, as far as the pages read
 * them: one start shortcut, two hold shortcuts, an end with each one's
 * combination, nothing own. */
export const defaultValues = (): Values => ({
  selected_model: "",
  selected_language: "auto",
  spoken_languages: [],
  translate_to_english: false,
  chinese_script: "as_transcribed",
  bindings: {
    paste_last_transcript: binding("paste_last_transcript", "ctrl+shift+v"),
    copy_last_transcript: binding("copy_last_transcript", "ctrl+shift+c"),
    cycle_word_list: binding("cycle_word_list", "ctrl+shift+l"),
    toggle_profile_auto: binding("toggle_profile_auto", "ctrl+shift+a"),
    cancel: binding("cancel", "escape"),
  },
  // Out of the box (qa 2026-11-26_1810, question 6 a): ⌃ Space starts when
  // tapped and records while held; ⌃⇧ Space records while held and
  // post-processes.
  start_keys: [startKey("start", "ctrl+space")],
  hold_keys: [
    endKey("hold_direct", "ctrl+space"),
    endKey("hold_post", "ctrl+shift+space", { post_process: true }),
  ],
  end_keys: [
    endKey("plain", "ctrl+space"),
    endKey("post_process", "ctrl+shift+space", { post_process: true }),
  ],
  word_lists: [],
  active_word_list_id: null,
  custom_words: [],
  profile_words: [],
  post_process_enabled: false,
  post_process_provider_id: "openai",
  post_process_providers: [],
  post_process_models: {},
  post_process_efforts: {},
  post_process_api_keys: {},
  post_process_prompt_order: [],
  post_process_timeout_secs: 30,
  audio_feedback: false,
  audio_feedback_volume: 1,
  sound_theme: "marimba",
  done_sound: true,
  mute_while_recording: false,
  always_on_microphone: false,
  selected_microphone: null,
  selected_channel: null,
  clamshell_microphone: null,
  selected_output_device: null,
  airpods_mode: false,
  overlay_position: "bottom",
  overlay_style: "minimal",
  paste_method: "ctrl_v",
  clipboard_handling: "dont_modify",
  clipboard_notice_ms: 0,
  typing_tool: "auto",
  external_script_path: null,
  filler_word_removal_enabled: false,
  custom_filler_words: null,
  word_list_llm_check: false,
  vad_enabled: true,
  long_form_chunking: false,
  context_age_limit_minutes: 10,
  context_max_entries: 3,
  context_char_budget: 4000,
  context_excluded_apps: [],
});

export interface ProfileSpec {
  id: string;
  name: string;
  /** Values that differ from the defaults (the profile's own ones). */
  values?: Values;
  /** The fields it holds as its own. */
  own?: string[];
  /** The entries it holds or switched off, per list field. */
  ownEntries?: Record<string, { own?: string[]; off?: string[] }>;
  apps?: string[];
}

/** A profile with all its values: the defaults', with `values` on top. */
export const profile = (spec: ProfileSpec, defaults: Values): Profile =>
  ({
    id: spec.id,
    name: spec.name,
    values: { ...defaults, ...spec.values },
    apps: spec.apps ?? [],
    own: spec.own ?? [],
    own_entries: spec.ownEntries ?? {},
  }) as unknown as Profile;

export interface SettingsSpec {
  /** Changes to the defaults' values. */
  defaults?: Values;
  profiles?: ProfileSpec[];
  /** The profile in use; the defaults when missing. */
  activeProfileId?: string;
  /** Settings of the application (no profile value). */
  app?: Partial<AppSettings>;
}

/** Settings as `get_app_settings` gives them. */
export const settingsWith = (spec: SettingsSpec = {}): AppSettings => {
  const defaults = { ...defaultValues(), ...spec.defaults };
  const profiles = (spec.profiles ?? []).map((p) => profile(p, defaults));
  const active = profiles.find((p) => p.id === spec.activeProfileId);
  const inUse = active ? (active.values as unknown as Values) : defaults;
  return {
    ...inUse,
    app_language: "en",
    keyboard_implementation: "tauri",
    onboarding_completed: true,
    debug_mode: false,
    post_process_prompts: [],
    profile_auto_by_app: true,
    ...spec.app,
    profiles,
    active_profile_id: active?.id ?? "default",
    default_values: defaults,
  } as unknown as AppSettings;
};
