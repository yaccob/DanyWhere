// DanyWhere: one setting of a page, with what the page's scope adds to it
// (qa 2026-11-19_1711, sketches 2 and 3 and the settings view):
//
// - in a profile (the profile view, P4, P7.3): the chain and the value's
//   state under the control: coupled (whole chain, grey "Default"), own and
//   different (broken chain in petrol, "own value · Default: …", Reset), own
//   and equal ("own value · same as default", Reset). A click on the whole
//   chain holds the value without changing it, a click on the broken one
//   couples it again. A list shows its own entries one by one (P10).
// - under the defaults (the settings view, P7.1, P7.2): the profiles that
//   deviate, each with its own control (the same component, in that
//   profile's scope) and a reset, or for a large setting a summary with
//   "Open in …"; the profiles that follow in one grey line; "+ Profile"
//   gives another profile its own value here, starting from the default.
//
// The same wrapper serves every setting: it knows the setting only by the
// serde names of its fields (and an entry), as the backend does.
import React from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, Link2, Link2Off, RotateCcw } from "lucide-react";
import { commands, type JsonValue } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { useProfiles } from "@/hooks/useProfiles";
import { Dropdown } from "@/components/ui/Dropdown";
import {
  combinedState,
  deviation,
  entriesOf,
  entryState,
  fieldState,
  ownValues,
  type EntryState,
  type LayeredProfile,
  type LayeredSettings,
  type ValueState,
} from "@/lib/profileScope";
import { fieldInfo, pageOfValue } from "@/lib/profileFields";
import { kindOf, useFieldKinds } from "./fieldKinds";
import { runCoupling } from "./profileWriter";
import {
  SettingRowOverrideProvider,
  SettingsScopeProvider,
  useOpenProfilePage,
  useSettingsScope,
} from "./SettingsScope";
import { useValueLabels } from "./useValueLabels";

export interface ScopedSettingProps {
  /** The profile value or values the setting shows (serde names). */
  field: string | readonly string[];
  /** With one list field: the entry the setting shows (a shortcut, a
   * provider's model). */
  entry?: string;
  /** What a deviating profile shows under the defaults instead of its own
   * control (P7.2); large settings of the registry get the entries or the
   * value as text when this is missing. */
  summary?: (values: Record<string, unknown>) => string;
  /** Under the defaults, a deviating profile shows its value as text, with
   * "Open in …", instead of its own control: for controls that get the
   * scope's values as props from around them (the provider's settings). */
  textOnly?: boolean;
  /** Shown in a profile only, never under the defaults. */
  profileOnly?: boolean;
  children: React.ReactNode;
}

const asList = (field: string | readonly string[]): readonly string[] =>
  typeof field === "string" ? [field] : field;

/** The raw settings: the defaults and every profile, unscoped. */
const useLayered = (): LayeredSettings | null =>
  useSettingsStore((s) => s.settings as unknown as LayeredSettings | null);

const stateOfEntry = (state: EntryState): ValueState =>
  state === "coupled" ? "coupled" : state === "own_equal" ? "own_equal" : "own";

/** Couples the setting of `profileId` again: its fields, or its entry. */
const resetIn = (
  profileId: string,
  fields: readonly string[],
  entry: string | undefined,
) =>
  runCoupling(() =>
    entry !== undefined
      ? commands.resetProfileEntry(profileId, fields[0], entry)
      : commands.resetProfileValues(profileId, [...fields]),
  );

/** Holds the setting of `profileId` as its own without changing it (P4). */
const holdIn = (
  profile: LayeredProfile,
  fields: readonly string[],
  entry: string | undefined,
) =>
  runCoupling(async () => {
    if (entry !== undefined) {
      const value = entriesOf(
        kindOf(fields[0]),
        profile.values[fields[0]],
      ).find(([id]) => id === entry)?.[1];
      return commands.setProfileEntry(
        { kind: "profile", id: profile.id },
        fields[0],
        entry,
        (value ?? null) as JsonValue,
      );
    }
    for (const field of fields) {
      const result = await commands.holdProfileValue(profile.id, field);
      if (result.status === "error") return result;
    }
    return { status: "ok" as const, data: null };
  });

export const ScopedSetting: React.FC<ScopedSettingProps> = (props) => {
  const scope = useSettingsScope();
  const layered = useLayered();
  // Re-render once the backend said how the fields couple.
  useFieldKinds((s) => s.kinds);
  if (!scope || !layered?.default_values) return <>{props.children}</>;
  if (scope.kind === "profile") {
    const profile = layered.profiles?.find((p) => p.id === scope.id);
    if (!profile) return <>{props.children}</>;
    return (
      <InProfile
        {...props}
        profile={profile}
        defaults={layered.default_values}
      />
    );
  }
  if (props.profileOnly) return null;
  return (
    <UnderDefaults
      {...props}
      profiles={layered.profiles ?? []}
      defaults={layered.default_values}
    />
  );
};

// ---- the profile view ------------------------------------------------------

interface InProfileProps extends ScopedSettingProps {
  profile: LayeredProfile;
  defaults: Record<string, unknown>;
}

const InProfile: React.FC<InProfileProps> = ({
  field,
  entry,
  profile,
  defaults,
  children,
}) => {
  const fields = asList(field);
  const states = fields.map((name) =>
    entry !== undefined
      ? stateOfEntry(entryState(profile, defaults, name, kindOf(name), entry))
      : fieldState(profile, defaults, name, kindOf(name)),
  );
  const state = combinedState(states);
  const own = state !== "coupled";
  return (
    <div
      className={`border-s-2 ${own ? "border-logo-primary" : "border-transparent"}`}
    >
      {children}
      <StateLine
        fields={fields}
        entry={entry}
        state={state}
        profile={profile}
        defaults={defaults}
      />
    </div>
  );
};

interface StateLineProps {
  fields: readonly string[];
  entry?: string;
  state: ValueState;
  profile: LayeredProfile;
  defaults: Record<string, unknown>;
}

const StateLine: React.FC<StateLineProps> = ({
  fields,
  entry,
  state,
  profile,
  defaults,
}) => {
  const { t } = useTranslation();
  const labels = useValueLabels();
  const own = state !== "coupled";
  const single = fields.length === 1 ? fields[0] : null;
  const isList =
    single !== null && entry === undefined && kindOf(single).kind !== "value";

  // "Default: …" for the value, the entry, or each own value that differs.
  const defaultValue =
    state === "own" ? labels.defaultText(fields, entry, profile) : null;
  const defaultText =
    defaultValue === null
      ? null
      : t("settings.profileView.state.defaultIs", { value: defaultValue });

  // A list's own entries, one by one (P10).
  const ownEntries =
    isList && single !== null
      ? ownValues(profile).filter((value) => value.field === single)
      : [];

  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 px-4 pb-2 text-xs">
      <ChainButton
        own={own}
        onClick={() =>
          own
            ? void resetIn(profile.id, fields, entry)
            : void holdIn(profile, fields, entry)
        }
      />
      {own ? (
        <span className="font-semibold text-logo-primary">
          {t("settings.profileView.state.own")}
        </span>
      ) : (
        <span className="text-ink-3">
          {t("settings.profileView.state.coupled")}
        </span>
      )}
      {defaultText && <span className="text-ink-3">· {defaultText}</span>}
      {state === "own_equal" && (
        <span className="text-ink-3">
          · {t("settings.profileView.state.sameAsDefault")}
        </span>
      )}
      {ownEntries.map((value) => {
        const entryValue = entriesOf(
          kindOf(value.field),
          value.off ? defaults[value.field] : profile.values[value.field],
        ).find(([id]) => id === value.entry)?.[1];
        const name = labels.entryName(
          value.field,
          value.entry ?? "",
          entryValue,
        );
        return (
          <span
            key={`${value.entry}-${value.off ? "off" : "own"}`}
            className="inline-flex items-center gap-1 rounded-full bg-logo-primary/10 ps-2 pe-1 py-0.5"
          >
            <span className={value.off ? "line-through text-ink-3" : ""}>
              {name}
            </span>
            <span className="text-ink-3">
              {value.off
                ? t("settings.profileView.state.entryOff")
                : t("settings.profileView.state.entryOwn")}
            </span>
            <button
              type="button"
              onClick={() =>
                void resetIn(profile.id, [value.field], value.entry)
              }
              title={t("settings.profileView.resetTooltip")}
              aria-label={t("settings.profileView.resetEntry", { name })}
              className="rounded-full p-0.5 text-ink-3 hover:text-logo-primary cursor-pointer"
            >
              <RotateCcw className="size-3" aria-hidden="true" />
            </button>
          </span>
        );
      })}
      {own && (
        <button
          type="button"
          onClick={() => void resetIn(profile.id, fields, entry)}
          title={t("settings.profileView.resetTooltip")}
          className="ms-auto inline-flex items-center gap-1 rounded-md border border-line px-2 py-0.5 hover:border-logo-primary hover:bg-logo-primary/10 cursor-pointer"
        >
          <RotateCcw className="size-3" aria-hidden="true" />
          {t("settings.profileView.reset")}
        </button>
      )}
    </div>
  );
};

/** The chain: whole while coupled, broken in petrol for an own value. A
 * click holds a coupled value or couples an own one again (P4). */
export const ChainButton: React.FC<{ own: boolean; onClick: () => void }> = ({
  own,
  onClick,
}) => {
  const { t } = useTranslation();
  const label = own
    ? t("settings.profileView.chain.couple")
    : t("settings.profileView.chain.hold");
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`rounded p-0.5 cursor-pointer hover:bg-logo-primary/10 ${
        own ? "text-logo-primary" : "text-ink-3"
      }`}
    >
      {own ? (
        <Link2Off className="size-4" aria-hidden="true" />
      ) : (
        <Link2 className="size-4" aria-hidden="true" />
      )}
    </button>
  );
};

// ---- the settings view under the defaults ----------------------------------

interface UnderDefaultsProps extends ScopedSettingProps {
  profiles: readonly LayeredProfile[];
  defaults: Record<string, unknown>;
}

const UnderDefaults: React.FC<UnderDefaultsProps> = ({
  field,
  entry,
  summary,
  textOnly = false,
  profiles,
  defaults,
  children,
}) => {
  const { t } = useTranslation();
  const { displayName } = useProfiles();
  const openProfilePage = useOpenProfilePage();
  const { valueLabel, entryName, fieldLabel } = useValueLabels();
  if (profiles.length === 0) return <>{children}</>;

  const fields = asList(field);
  const deviating = new Set<string>();
  for (const name of fields) {
    for (const profile of deviation(profiles, name, entry).own) {
      deviating.add(profile.id);
    }
  }
  const own = profiles.filter((p) => deviating.has(p.id));
  const following = profiles.filter((p) => !deviating.has(p.id));
  const large =
    summary !== undefined ||
    textOnly ||
    (fields.length === 1 && entry === undefined && fieldInfo(fields[0])?.large);
  const page = pageOfValue(fields[0], entry);

  const summaryOf = (profile: LayeredProfile): string => {
    if (summary) return summary(profile.values);
    // Several values: the own ones, each with its name.
    if (fields.length > 1) {
      return fields
        .filter((name) => deviation([profile], name).own.length > 0)
        .map(
          (name) =>
            `${fieldLabel(name)}: ${valueLabel(name, profile.values[name], profile.values)}`,
        )
        .join(" · ");
    }
    const single = fields[0];
    const kind = kindOf(single);
    // One entry: its value.
    if (entry !== undefined) {
      const value = entriesOf(kind, profile.values[single]).find(
        ([id]) => id === entry,
      )?.[1];
      if (value === undefined) return t("settings.profileView.state.entryOff");
      if (typeof value === "object" && value !== null) {
        // A provider's settings: its address; another entry: its name.
        const address = (value as Record<string, unknown>).base_url;
        return typeof address === "string" && address
          ? address
          : entryName(single, entry, value);
      }
      return valueLabel(single, value, profile.values);
    }
    if (kind.kind === "value")
      return valueLabel(single, profile.values[single], profile.values);
    const mine = ownValues(profile).filter((v) => v.field === single);
    const names = (off: boolean) =>
      mine
        .filter((v) => Boolean(v.off) === off)
        .map((v) =>
          entryName(
            single,
            v.entry ?? "",
            entriesOf(
              kind,
              off ? defaults[single] : profile.values[single],
            ).find(([id]) => id === v.entry)?.[1],
          ),
        );
    const parts: string[] = [];
    const ownNames = names(false);
    const offNames = names(true);
    if (ownNames.length) {
      parts.push(
        t("settings.profileView.state.ownEntries", {
          names: ownNames.join(", "),
        }),
      );
    }
    if (offNames.length) {
      parts.push(
        t("settings.profileView.state.offEntries", {
          names: offNames.join(", "),
        }),
      );
    }
    return parts.join(" · ");
  };

  const followNames = following.map((p) => displayName(p)).join(", ");
  const followText =
    own.length === 0
      ? t("settings.profileView.deviations.allFollow")
      : following.length === 1
        ? t("settings.profileView.deviations.followsOne", {
            names: followNames,
          })
        : t("settings.profileView.deviations.followMany", {
            names: followNames,
          });

  return (
    <div>
      {children}
      <div className="flex flex-col gap-1 ps-8 pe-4 pb-2">
        {own.map((profile) => {
          const tag = (
            <span className="inline-flex min-w-0 items-center gap-1 font-medium text-logo-primary">
              <Link2Off className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="truncate">{displayName(profile)}</span>
            </span>
          );
          const reset = (
            <button
              type="button"
              onClick={() => void resetIn(profile.id, fields, entry)}
              title={t("settings.profileView.deviations.resetFor", {
                name: displayName(profile),
              })}
              aria-label={t("settings.profileView.deviations.resetFor", {
                name: displayName(profile),
              })}
              className="shrink-0 rounded p-1 text-ink-3 hover:text-logo-primary hover:bg-logo-primary/10 cursor-pointer"
            >
              <RotateCcw className="size-3.5" aria-hidden="true" />
            </button>
          );
          if (large) {
            return (
              <div
                key={profile.id}
                className="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm"
              >
                {tag}
                <span className="min-w-0 flex-1 truncate text-xs text-ink-3">
                  {summaryOf(profile)}
                </span>
                {openProfilePage && (
                  <button
                    type="button"
                    onClick={() => openProfilePage(profile.id, page)}
                    className="inline-flex shrink-0 items-center gap-1 rounded-md border border-line px-2 py-0.5 text-xs hover:border-logo-primary hover:bg-logo-primary/10 cursor-pointer"
                  >
                    <ArrowRight
                      className="size-3 rtl:rotate-180"
                      aria-hidden="true"
                    />
                    {t("settings.profileView.deviations.openIn", {
                      name: displayName(profile),
                    })}
                  </button>
                )}
                {reset}
              </div>
            );
          }
          return (
            <div key={profile.id} className="flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <SettingsScopeProvider
                  scope={{ kind: "profile", id: profile.id }}
                >
                  <SettingRowOverrideProvider value={{ title: tag }}>
                    {children}
                  </SettingRowOverrideProvider>
                </SettingsScopeProvider>
              </div>
              {reset}
            </div>
          );
        })}
        <div className="flex flex-wrap items-center gap-2 text-xs text-ink-3">
          <Link2 className="size-3.5 shrink-0" aria-hidden="true" />
          <span className="min-w-0 flex-1">{followText}</span>
          {following.length > 0 && (
            <Dropdown
              compact
              tone="neutral"
              className="shrink-0"
              options={following.map((profile) => ({
                value: profile.id,
                label: displayName(profile),
              }))}
              selectedValue={null}
              placeholder={t("settings.profileView.deviations.add")}
              onSelect={(id) => {
                const profile = following.find((p) => p.id === id);
                if (profile) void holdIn(profile, fields, entry);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
};
