/**
 * DanyWhere: names and short texts of a profile's values, for the state
 * line ("Default: …"), the page "Profile", the quiet bar, the confirmations
 * and the settings view (qa 2026-11-19_1711 P4, P5, P7, P8). Generic: a
 * field's name comes from the registry, an entry's from its own name (or
 * the defaults' entry of that id, for one a profile switched off), a
 * value's text from `valueText`, with the label its control shows where
 * the value is a choice, a reference or a shortcut. Pure: `useValueLabels`
 * gives it the translations and what the app knows (models, providers,
 * prompts, the defaults).
 */
import type { EndKey } from "../bindings";
import { deviceLabel, providerName } from "./displayNames";
import { START_BINDINGS, defaultEndOf, endKeyName } from "./endKeys";
import { fieldInfo } from "./profileFields";
import {
  entryLabel,
  entriesOf,
  entryState,
  fieldState,
  valueText,
  type FieldKind,
  type LayeredProfile,
  type OwnValue,
} from "./profileScope";

export type Translate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

type Values = Readonly<Record<string, unknown>>;

export interface LabelContext {
  t: Translate;
  /** Whether a translation key exists. */
  exists: (key: string) => boolean;
  kindOf: (field: string) => FieldKind;
  /** The defaults' values: names of entries a profile switched off, and
   * what a value refers to when nothing else says. */
  defaults: Values;
  models?: readonly { id: string; name: string }[];
  providers?: readonly { id: string; label: string }[];
  prompts?: readonly { id: string; name: string }[];
  /** A stored key combination as the shortcut controls show it. */
  formatShortcut?: (binding: string) => string;
  /** A language code's name. */
  languageName?: (code: string) => string;
  /** Placeholders the options' labels take (the paste method's modifier). */
  optionParams?: Record<string, unknown>;
}

/** The name the device lists give the system's default device (backend
 * `commands/audio.rs`); a profile stores it as nothing or "default". It is
 * shown translated, as the lists show it (`deviceLabel`). */
export const DEFAULT_DEVICE = "Default";

/** Values that are never shown, only whether they are set. */
const SECRET_FIELDS = new Set(["post_process_api_keys"]);

/** Fields whose entries are named after a post-processing provider. */
const PROVIDER_MAPS = new Set([
  "post_process_models",
  "post_process_efforts",
  "post_process_api_keys",
  "post_process_providers",
]);

/** Maps by provider whose missing entry is a value, not "off": no effort
 * level is sent (the model's default), no model, no key. */
const ABSENT_IS_EMPTY = new Set([
  "post_process_models",
  "post_process_efforts",
  "post_process_api_keys",
]);

/** Fields that name a model, by its id. */
const MODEL_FIELDS = new Set(["selected_model", "post_process_models"]);

/** Fields that name an audio device. */
const DEVICE_FIELDS = new Set([
  "selected_microphone",
  "selected_output_device",
  "clamshell_microphone",
]);

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isEmpty = (value: unknown) =>
  value === undefined || value === null || value === "";

const camel = (text: string) =>
  text.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase());

export const fieldLabel = (ctx: LabelContext, field: string): string => {
  const info = fieldInfo(field);
  return info ? ctx.t(info.label) : field;
};

/** Entry `id` of list or map `field`: from `within` (a profile's values),
 * else from the defaults, which still have one a profile switched off. */
export const findEntry = (
  ctx: LabelContext,
  field: string,
  id: string,
  within?: Values,
): unknown => {
  const kind = ctx.kindOf(field);
  for (const values of [within, ctx.defaults]) {
    if (!values) continue;
    const found = entriesOf(kind, values[field]).find(
      ([entryId]) => entryId === id,
    );
    if (found) return found[1];
  }
  return undefined;
};

/** What to call entry `id` of list or map `field`; `entry` is the entry
 * itself when the caller has it, else it is looked up in `within` and the
 * defaults. */
export const entryName = (
  ctx: LabelContext,
  field: string,
  id: string,
  entry?: unknown,
  within?: Values,
): string => {
  const stored = entry ?? findEntry(ctx, field, id, within);
  if (field === "bindings" || field === "default_ends") {
    const own = isObject(stored) ? entryLabel(stored, id) : id;
    return ctx.t(`settings.general.shortcut.bindings.${id}.name`, {
      defaultValue: own,
    });
  }
  if (PROVIDER_MAPS.has(field)) {
    const known = ctx.providers?.find((p) => p.id === id);
    return providerName(
      known ?? {
        id,
        label: entryLabel(
          findEntry(ctx, "post_process_providers", id, within),
          id,
        ),
      },
      ctx.t,
    );
  }
  if (field === "end_keys") {
    if (isObject(stored) && typeof stored.name === "string") {
      return endKeyName(stored as unknown as EndKey, ctx.t) || id;
    }
    return ctx.t(`settings.endKeys.builtin.${id}`, { defaultValue: id });
  }
  return entryLabel(stored, id);
};

/** The label of choice `value` of `field`, as its dropdown shows it; null
 * when `field` has no labelled choices or `value` is none of them. */
const optionLabel = (
  ctx: LabelContext,
  field: string,
  value: unknown,
): string | null => {
  const info = fieldInfo(field);
  if (!info) return null;
  const options = info.options;
  let key: string | null = null;
  if (options && isEmpty(value) && options.empty) {
    key = `${options.prefix}.${options.empty}`;
  } else if (typeof value === "string" && value) {
    const name = options?.keys?.[value] ?? camel(value);
    // Without its own prefix, the options follow the setting's name
    // ("…pasteMethod.title" → "…pasteMethod.options.direct").
    const prefix =
      options?.prefix ??
      `${info.label.replace(/\.(title|label)$/, "")}.options`;
    key = `${prefix}.${name}`;
  }
  if (key === null || !ctx.exists(key)) return null;
  const text = ctx.t(key, ctx.optionParams ?? {});
  return typeof text === "string" && !text.includes("{{") ? text : null;
};

/** The prompts an order runs, by name: "A, B" (the setting is called
 * "Active Prompts"), or "no prompt active". */
const promptOrderLabel = (ctx: LabelContext, value: unknown[]): string => {
  const names = value
    .filter((item) => isObject(item) && item.active === true)
    .map((item) => {
      const id = isObject(item) ? item.id : undefined;
      return ctx.prompts?.find((prompt) => prompt.id === id)?.name;
    })
    .filter((name): name is string => Boolean(name));
  return names.length > 0
    ? names.join(", ")
    : ctx.t("settings.profileView.summary.noneActive");
};

/** Each start shortcut with the end it ends with by default. */
const defaultEndsLabel = (
  ctx: LabelContext,
  value: Record<string, unknown>,
  within: Values,
): string =>
  START_BINDINGS.map((binding) => {
    const endId = defaultEndOf(
      binding,
      value as Partial<Record<string, string>>,
    );
    return `${entryName(ctx, "bindings", binding, undefined, within)}: ${entryName(
      ctx,
      "end_keys",
      endId,
      undefined,
      within,
    )}`;
  }).join(" · ");

/** The text of `value` of `field` (or of one of its entries); `within` are
 * the values it belongs to (a profile's, else the defaults'), for what it
 * refers to (a word list by its id). */
export const valueLabel = (
  ctx: LabelContext,
  field: string,
  value: unknown,
  within: Values = ctx.defaults,
): string => {
  const { t } = ctx;
  if (SECRET_FIELDS.has(field)) {
    return value
      ? t("settings.profileView.value.hidden")
      : t("settings.profileView.value.notSet");
  }
  if (
    DEVICE_FIELDS.has(field) &&
    (isEmpty(value) || value === "default" || value === DEFAULT_DEVICE)
  ) {
    return deviceLabel(DEFAULT_DEVICE, t);
  }
  // The chosen provider by the name its list shows, not by its id.
  if (
    field === "post_process_provider_id" &&
    typeof value === "string" &&
    value
  ) {
    const known = ctx.providers?.find((p) => p.id === value);
    return providerName(known ?? { id: value, label: value }, t);
  }
  // A string behind a switch shows as the switch does; a fixed name as its
  // control shows it (qa 2026-11-22_1731).
  const info = fieldInfo(field);
  if (info?.switchOn !== undefined && typeof value === "string") {
    return t(
      `settings.profileView.value.${value === info.switchOn ? "on" : "off"}`,
    );
  }
  if (typeof value === "string" && info?.names) {
    return info.names(value, t);
  }
  const option = optionLabel(ctx, field, value);
  if (option !== null) return option;
  if (MODEL_FIELDS.has(field) && isEmpty(value)) {
    return t("settings.profileView.value.noModel");
  }
  if (field === "selected_model" && typeof value === "string" && value) {
    return ctx.models?.find((m) => m.id === value)?.name ?? value;
  }
  // The channel list counts from 1; no channel averages them all (the
  // registry's empty option).
  if (field === "selected_channel" && typeof value === "number") {
    return t("settings.sound.channel.channel", { n: value + 1 });
  }
  // No own list removes the built-in filler words; an empty one, none.
  if (
    field === "custom_filler_words" &&
    (value === null || value === undefined)
  ) {
    return t("settings.profileView.value.builtInFillerWords");
  }
  if (field === "selected_language" && value === "auto") {
    return t("settings.general.language.auto");
  }
  if (
    (field === "selected_language" || field === "spoken_languages") &&
    ctx.languageName
  ) {
    const name = ctx.languageName;
    if (typeof value === "string" && value) return name(value);
    if (Array.isArray(value) && value.length > 0) {
      return value.map((code) => name(String(code))).join(", ");
    }
  }
  if (field === "active_word_list_id") {
    return typeof value === "string" && value
      ? entryName(ctx, "word_lists", value, undefined, within)
      : t("settings.advanced.wordLists.onlyGlobal");
  }
  if (field === "post_process_prompt_order" && Array.isArray(value)) {
    return promptOrderLabel(ctx, value);
  }
  if (field === "default_ends" && isObject(value)) {
    return defaultEndsLabel(ctx, value, within);
  }
  if (
    field === "bindings" &&
    isObject(value) &&
    typeof value.current_binding === "string"
  ) {
    return value.current_binding && ctx.formatShortcut
      ? ctx.formatShortcut(value.current_binding)
      : value.current_binding || t("settings.profileView.value.empty");
  }
  const shown = valueText(value);
  if ("text" in shown) return shown.text;
  if (shown.key === "items") {
    return t("settings.profileView.value.items", { number: shown.count });
  }
  return t(`settings.profileView.value.${shown.key}`);
};

/** "Setting" or "Setting: entry" for an own value of a profile whose
 * values are `within`. */
export const ownValueLabel = (
  ctx: LabelContext,
  value: OwnValue,
  within: Values,
): string =>
  value.entry === undefined
    ? fieldLabel(ctx, value.field)
    : `${fieldLabel(ctx, value.field)}: ${entryName(
        ctx,
        value.field,
        value.entry,
        undefined,
        value.off ? ctx.defaults : within,
      )}`;

/** The text of an entry of `field` that a profile switched off: in a map
 * by provider what a missing entry means there (the model's default
 * effort, no model, no key), else "off". */
export const offEntryText = (ctx: LabelContext, field: string): string =>
  ABSENT_IS_EMPTY.has(field)
    ? valueLabel(ctx, field, undefined)
    : ctx.t("settings.profileView.state.entryOff");

/** A row of the page "Profile": "Setting: value". An entry of a list (a
 * word list, an end shortcut) is named, not shown; an entry of a map (a
 * shortcut, a provider's model) shows its value; a switched-off one says
 * so, or what a missing one means for a provider. */
export const ownValueRow = (
  ctx: LabelContext,
  value: OwnValue,
  within: Values,
): string => {
  const label = ownValueLabel(ctx, value, within);
  if (value.off) {
    return ABSENT_IS_EMPTY.has(value.field)
      ? `${label}: ${offEntryText(ctx, value.field)}`
      : `${label} (${offEntryText(ctx, value.field)})`;
  }
  if (value.entry === undefined) {
    return `${label}: ${valueLabel(ctx, value.field, within[value.field], within)}`;
  }
  if (ctx.kindOf(value.field).kind === "list") return label;
  const current = entriesOf(ctx.kindOf(value.field), within[value.field]).find(
    ([id]) => id === value.entry,
  )?.[1];
  return `${label}: ${valueLabel(ctx, value.field, current, within)}`;
};

/** What "Default: …" names under a setting of `fields` (or its entry
 * `entry`) in `profile`: the default's value of the one field, or of each
 * own field that differs, by name; null when nothing differs and for whole
 * lists, whose own entries show one by one. */
export const defaultText = (
  ctx: LabelContext,
  fields: readonly string[],
  entry: string | undefined,
  profile: LayeredProfile,
): string | null => {
  const { defaults } = ctx;
  if (entry !== undefined) {
    const field = fields[0];
    const kind = ctx.kindOf(field);
    // An entry the profile switched off (e.g. the effort of a model that
    // has none) still names the default's entry, as an own one does.
    const state = entryState(profile, defaults, field, kind, entry);
    if (state !== "own" && state !== "off") return null;
    const base = entriesOf(kind, defaults[field]).find(
      ([id]) => id === entry,
    )?.[1];
    return valueLabel(ctx, field, base);
  }
  const differing = fields.filter((field) => {
    const kind = ctx.kindOf(field);
    return (
      kind.kind === "value" &&
      fieldState(profile, defaults, field, kind) === "own"
    );
  });
  if (differing.length === 0) return null;
  if (fields.length === 1) {
    return valueLabel(ctx, differing[0], defaults[differing[0]]);
  }
  return differing
    .map(
      (field) =>
        `${fieldLabel(ctx, field)}: ${valueLabel(ctx, field, defaults[field])}`,
    )
    .join(" · ");
};
