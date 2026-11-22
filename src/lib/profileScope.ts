/**
 * DanyWhere: the defaults and the profiles that override them, as the
 * settings window reads them (qa 2026-11-19_1711 P4, P5, P7, P8, P10; the
 * backend is `src-tauri/src/profile_layers.rs`).
 *
 * The defaults ("Grundeinstellungen", `AppSettings.default_values`) hold a
 * value for every field of a profile's values. A profile holds each field
 * coupled (it follows the default) or as its own value (`Profile.own`);
 * lists and maps deviate entry by entry (`Profile.own_entries`). A profile's
 * `values` are always complete, so reading a value needs no merging here:
 * this module only tells states apart, counts and maps settings to fields.
 * Everything is pure and works on the serde names of the fields, so it
 * needs no code per setting.
 */

/** Where a page reads and writes values (backend `profile_layers::Scope`). */
export type Scope = { kind: "defaults" } | { kind: "profile"; id: string };

export const DEFAULTS_SCOPE: Scope = { kind: "defaults" };

/** How a field couples (backend `profile_layers::FieldKind`). */
export type FieldKind =
  | { kind: "value" }
  | { kind: "list"; key: string }
  | { kind: "map" };

/** A profile's own entries of one list field (backend `OwnEntries`). */
export interface OwnEntriesLike {
  own?: readonly string[] | null;
  off?: readonly string[] | null;
}

/** A profile as this module needs it; `own` and `own_entries` are missing
 * in settings from before the defaults, which counts as fully coupled. */
export interface LayeredProfile {
  id: string;
  name: string;
  values: Record<string, unknown>;
  own?: readonly string[] | null;
  own_entries?: Partial<Record<string, OwnEntriesLike>> | null;
}

export interface LayeredSettings {
  profiles?: readonly LayeredProfile[] | null;
  default_values?: Record<string, unknown> | null;
}

/**
 * A value's state in a profile (P4): `coupled` follows the default; `own`
 * stays when the default changes; `own_equal` is own and happens to equal
 * the default now.
 */
export type ValueState = "coupled" | "own" | "own_equal";

/** An entry's state: as a value's, or `added` (the profile's own entry the
 * defaults don't have) or `off` (an entry of the defaults switched off). */
export type EntryState = ValueState | "added" | "off";

/** One own value of a profile: a field, or an entry of a list field. */
export interface OwnValue {
  field: string;
  entry?: string;
  /** An entry of the defaults the profile switched off. */
  off?: boolean;
}

/** A short text for a value, or what to translate instead. */
export type ValueText =
  | { text: string }
  | { key: "on" | "off" | "empty" }
  | { key: "items"; count: number };

// ---- comparing -------------------------------------------------------------

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Whether two values are the same, comparing by content: objects ignore
 * the order of their keys, `null` and `undefined` both mean nothing (as the
 * backend's JSON compares them). */
export const sameValue = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;
  if (a === null || a === undefined) return b === null || b === undefined;
  if (Array.isArray(a)) {
    return (
      Array.isArray(b) &&
      a.length === b.length &&
      a.every((item, index) => sameValue(item, b[index]))
    );
  }
  if (isObject(a)) {
    if (!isObject(b)) return false;
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      if (!sameValue(a[key], b[key])) return false;
    }
    return true;
  }
  return false;
};

/** The entries of a list or map value by id, in order; none for a value. */
export const entriesOf = (
  kind: FieldKind,
  value: unknown,
): [string, unknown][] => {
  if (kind.kind === "list" && Array.isArray(value)) {
    const result: [string, unknown][] = [];
    for (const item of value) {
      const id = isObject(item) ? item[kind.key] : undefined;
      if (typeof id === "string") result.push([id, item]);
    }
    return result;
  }
  if (kind.kind === "map" && isObject(value)) {
    return Object.entries(value);
  }
  return [];
};

const entryOf = (kind: FieldKind, value: unknown, id: string): unknown =>
  entriesOf(kind, value).find(([entryId]) => entryId === id)?.[1];

// ---- states ------------------------------------------------------------------

const ownFields = (profile: LayeredProfile): readonly string[] =>
  profile.own ?? [];

const heldEntries = (
  profile: LayeredProfile,
  field: string,
): { own: readonly string[]; off: readonly string[] } => {
  const held = profile.own_entries?.[field];
  return { own: held?.own ?? [], off: held?.off ?? [] };
};

const holdsAnyEntry = (profile: LayeredProfile, field: string): boolean => {
  const { own, off } = heldEntries(profile, field);
  return own.length > 0 || off.length > 0;
};

/** Whether `profile` holds `field` (or its entry `entry`) as its own. */
const holds = (
  profile: LayeredProfile,
  field: string,
  entry?: string,
): boolean => {
  if (entry === undefined) {
    return ownFields(profile).includes(field) || holdsAnyEntry(profile, field);
  }
  const { own, off } = heldEntries(profile, field);
  return own.includes(entry) || off.includes(entry);
};

/** A whole field's state in `profile` against the `defaults`' values. */
export const fieldState = (
  profile: LayeredProfile,
  defaults: Record<string, unknown>,
  field: string,
  kind: FieldKind,
): ValueState => {
  const own =
    kind.kind === "value"
      ? ownFields(profile).includes(field)
      : holdsAnyEntry(profile, field);
  if (!own) return "coupled";
  return sameValue(profile.values[field], defaults[field])
    ? "own_equal"
    : "own";
};

/** The state of entry `entry` of list field `field` in `profile`. */
export const entryState = (
  profile: LayeredProfile,
  defaults: Record<string, unknown>,
  field: string,
  kind: FieldKind,
  entry: string,
): EntryState => {
  const { own, off } = heldEntries(profile, field);
  if (off.includes(entry)) return "off";
  if (!own.includes(entry)) return "coupled";
  const base = entryOf(kind, defaults[field], entry);
  if (base === undefined) return "added";
  return sameValue(entryOf(kind, profile.values[field], entry), base)
    ? "own_equal"
    : "own";
};

/** The state of a setting made of several fields: own as soon as one
 * differs, own and equal when the own ones all equal their defaults. */
export const combinedState = (states: ValueState[]): ValueState => {
  if (states.includes("own")) return "own";
  if (states.includes("own_equal")) return "own_equal";
  return "coupled";
};

/** Whether changing `field` (or its entry `entry`) in `profile` makes a
 * coupled value its own, which the quiet bar of P7.3 tells. A whole list
 * counts as coupled while the profile holds none of its entries. */
export const becomesOwn = (
  profile: LayeredProfile,
  field: string,
  kind: FieldKind,
  entry?: string,
): boolean => {
  if (entry !== undefined) return !holds(profile, field, entry);
  return kind.kind === "value"
    ? !ownFields(profile).includes(field)
    : !holdsAnyEntry(profile, field);
};

// ---- own values and counts (P5) ------------------------------------------

/** A profile's own values in the backend's order: its own fields, then per
 * list field (by name) its own entries and its switched-off ones. */
export const ownValues = (profile: LayeredProfile): OwnValue[] => {
  const result: OwnValue[] = ownFields(profile).map((field) => ({ field }));
  const lists = Object.keys(profile.own_entries ?? {}).sort();
  for (const field of lists) {
    const { own, off } = heldEntries(profile, field);
    for (const entry of own) result.push({ field, entry });
    for (const entry of off) result.push({ field, entry, off: true });
  }
  return result;
};

/** How many own values `profile` has (backend `profile_layers::own_count`). */
export const ownCount = (profile: LayeredProfile): number =>
  ownValues(profile).length;

/** How many of them are on `page`, as `pageOf` places a field or entry. */
export const ownCountOnPage = (
  profile: LayeredProfile,
  page: string,
  pageOf: (field: string, entry?: string) => string,
): number =>
  ownValues(profile).filter(
    (value) => pageOf(value.field, value.entry) === page,
  ).length;

// ---- who deviates (P7.1) -------------------------------------------------

/** The profiles that hold `field` (or its entry `entry`) as their own, and
 * those that follow the default there, each in their order. */
export const deviation = <P extends LayeredProfile>(
  profiles: readonly P[],
  field: string,
  entry?: string,
): { own: P[]; following: P[] } => {
  const own: P[] = [];
  const following: P[] = [];
  for (const profile of profiles) {
    (holds(profile, field, entry) ? own : following).push(profile);
  }
  return { own, following };
};

/** The line under a default that names the profiles following it, as a
 * translation key under `settings.profileView.deviations` with its names. */
export type FollowLine =
  | { key: "allFollow" }
  | { key: "followsOne" | "followMany"; names: string };

export const followLine = (
  ownCount: number,
  followingNames: readonly string[],
): FollowLine | null =>
  // Nobody follows (every profile deviates, or there is none): no line, and
  // no "+ profile" either, since there is no one left to give a value.
  followingNames.length === 0
    ? null
    : ownCount === 0
      ? { key: "allFollow" }
      : followingNames.length === 1
        ? { key: "followsOne", names: followingNames[0] }
        : { key: "followMany", names: followingNames.join(", ") };

// ---- scopes and the settings a page reads -----------------------------------

export const sameScope = (a: Scope, b: Scope): boolean =>
  a.kind === b.kind &&
  (a.kind === "defaults" || (b.kind === "profile" && a.id === b.id));

/** The values of a scope: the defaults' or a profile's; `null` for an
 * unknown profile, and for the defaults of settings from before them. */
export const scopeValues = (
  settings: LayeredSettings,
  scope: Scope,
): Record<string, unknown> | null => {
  if (scope.kind === "defaults") return settings.default_values ?? null;
  return (
    settings.profiles?.find((profile) => profile.id === scope.id)?.values ??
    null
  );
};

/** Fields of a profile's values that live inside `dictation_context` in the
 * flat settings (backend `ProfileValues::from_settings`). */
const CONTEXT_PARTS: Record<string, string> = {
  context_age_limit_minutes: "age_limit_minutes",
  context_max_entries: "max_entries",
  context_char_budget: "char_budget",
  context_excluded_apps: "excluded_apps",
};

/** The settings as a page in a scope reads them: the flat fields replaced
 * by the scope's `values` (which mirror them by name). `settings` stays. */
export const scopedSettings = <S extends object>(
  settings: S,
  values: Record<string, unknown>,
): S => {
  const result: Record<string, unknown> = {
    ...(settings as unknown as Record<string, unknown>),
  };
  const context: Record<string, unknown> = isObject(result.dictation_context)
    ? { ...result.dictation_context }
    : {};
  let contextChanged = false;
  for (const [field, value] of Object.entries(values)) {
    const part = CONTEXT_PARTS[field];
    if (part) {
      context[part] = value;
      contextChanged = true;
    } else {
      result[field] = value;
    }
  }
  if (contextChanged) result.dictation_context = context;
  return result as S;
};

/**
 * What changing setting `key` to `value` writes in a scope: the profile
 * fields with their new values; `null` when `key` is no profile value (it
 * changes as before, for the whole application). `current` is the
 * settings the page read (scoped); `fields` the names of a profile's values.
 */
export const profileWrites = (
  key: string,
  value: unknown,
  current: Record<string, unknown>,
  fields: ReadonlySet<string>,
): [string, unknown][] | null => {
  if (key === "dictation_context") {
    const before = isObject(current.dictation_context)
      ? current.dictation_context
      : {};
    const after = isObject(value) ? value : {};
    const writes: [string, unknown][] = [];
    for (const [field, part] of Object.entries(CONTEXT_PARTS)) {
      if (!fields.has(field)) continue;
      if (!sameValue(before[part], after[part])) {
        writes.push([field, after[part]]);
      }
    }
    return writes;
  }
  return fields.has(key) ? [[key, value]] : null;
};

/** `settings` with `field` set to `value` in `scope` (the defaults' values
 * or a profile's), for showing a change before the backend confirms it;
 * everything else is shared, nothing is changed in place. */
export const patchScope = <S extends LayeredSettings>(
  settings: S,
  scope: Scope,
  field: string,
  value: unknown,
): S => {
  if (scope.kind === "defaults") {
    if (!settings.default_values) return settings;
    return {
      ...settings,
      default_values: { ...settings.default_values, [field]: value },
    };
  }
  const profiles = settings.profiles ?? [];
  const index = profiles.findIndex((profile) => profile.id === scope.id);
  if (index < 0) return settings;
  const next = [...profiles];
  next[index] = {
    ...profiles[index],
    values: { ...profiles[index].values, [field]: value },
  };
  return { ...settings, profiles: next };
};

// ---- showing values ------------------------------------------------------------

const MAX_TEXT = 40;

const cut = (text: string): string =>
  text.length > MAX_TEXT ? `${text.slice(0, MAX_TEXT)}…` : text;

/** Control characters as the escapes the "Append" field shows. */
const visible = (text: string): string =>
  text.replace(/\n/g, "\\n").replace(/\t/g, "\\t").replace(/\r/g, "\\r");

/** A short text for a value (P4: "Grundwert: …"); lists of objects and
 * other structures only as a count, the page shows them in full. */
export const valueText = (value: unknown): ValueText => {
  if (value === null || value === undefined || value === "") {
    return { key: "empty" };
  }
  if (typeof value === "boolean") return { key: value ? "on" : "off" };
  if (typeof value === "number") return { text: String(value) };
  if (typeof value === "string") return { text: cut(visible(value)) };
  if (Array.isArray(value)) {
    if (value.length === 0) return { key: "empty" };
    if (value.every((item) => typeof item === "string")) {
      return { text: cut(value.map((item) => visible(item)).join(", ")) };
    }
    return { key: "items", count: value.length };
  }
  if (isObject(value)) {
    const count = Object.keys(value).length;
    return count === 0 ? { key: "empty" } : { key: "items", count };
  }
  return { key: "empty" };
};

/** What to call an entry of a list: its `name`, else its `label`, else
 * its id (a map's entries are named by their key). */
export const entryLabel = (entry: unknown, id: string): string => {
  if (isObject(entry)) {
    for (const key of ["name", "label"]) {
      const text = entry[key];
      if (typeof text === "string" && text.trim()) return text.trim();
    }
  }
  return id;
};
