import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { commands, type EndKey } from "@/bindings";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "@/hooks/useSettings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Dropdown } from "../../ui/Dropdown";
import { EndKeysEditor } from "./EndKeysEditor";
// DanyWhere (strand E2): the end shortcuts of the page's scope, the defaults
// or a profile, each with its chain.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { useSettingsScope } from "../profiles/scope/SettingsScope";
import { writeValue } from "../profiles/scope/profileWriter";
import {
  START_BINDINGS,
  defaultEndOf,
  endKeyName,
  endKeyErrorKey,
  endKeyOptions,
} from "@/lib/endKeys";

interface DefaultEndSelectProps {
  /** The start shortcut's binding id. */
  binding: string;
  /** The end shortcut it ends with by default. */
  value: string;
  endKeys: EndKey[];
  onChange: (endId: string) => Promise<void> | void;
  disabled?: boolean;
}

/**
 * DanyWhere: how a start shortcut's second press, or its release when held,
 * ends the recording (end_keys.rs). Controlled, like the list.
 */
export const DefaultEndSelect: React.FC<DefaultEndSelectProps> = ({
  binding,
  value,
  endKeys,
  onChange,
  disabled = false,
}) => {
  const { t } = useTranslation();
  return (
    <SettingContainer
      title={t("settings.endKeys.defaultEnd.title", {
        shortcut: t(`settings.general.shortcut.bindings.${binding}.name`),
      })}
      description={t("settings.endKeys.defaultEnd.description")}
      descriptionMode="tooltip"
      grouped={true}
      layout="horizontal"
    >
      <Dropdown
        compact
        className="w-48"
        options={endKeyOptions(endKeys, t)}
        selectedValue={value}
        onSelect={(endId) => void onChange(endId)}
        disabled={disabled}
      />
    </SettingContainer>
  );
};

/**
 * DanyWhere: the end shortcuts on the shortcut settings (end_keys.rs): the
 * default end of each start shortcut and the list, of the page's scope (the
 * defaults or a profile; outside one, the values in use).
 */
export const EndKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [busy, setBusy] = useState(false);
  const endKeys = getSetting("end_keys") ?? [];
  const defaultEnds = getSetting("default_ends") ?? {};
  const isLinux = type() === "linux";
  const scope = useSettingsScope();

  /** A profile's default ends under the defaults, as text (P7.2): the
   * selections get the page's values as props. */
  const defaultEndsSummary = (values: Record<string, unknown>) => {
    const ends = (values.default_ends ?? {}) as Partial<Record<string, string>>;
    const keys = (values.end_keys ?? []) as EndKey[];
    return START_BINDINGS.map((binding) => {
      const endId = defaultEndOf(binding, ends);
      const end = keys.find((key) => key.id === endId);
      return `${t(`settings.general.shortcut.bindings.${binding}.name`)}: ${
        end ? endKeyName(end, t) : endId
      }`;
    }).join(" · ");
  };

  /** A refusal of the backend as the notice says it. */
  const describe = (error: string): string => {
    const key = endKeyErrorKey(error);
    const conflict = /conflict:([a-z_]+)/.exec(error);
    return key
      ? t(key)
      : conflict
        ? t("settings.general.insertService.conflict", {
            name: t(
              `settings.general.shortcut.bindings.${conflict[1]}.name`,
              conflict[1],
            ),
          })
        : t("settings.endKeys.errors.save", { error });
  };

  /** Writes `field` into the page's scope. */
  const save = async (field: string, value: unknown) => {
    if (!scope) return;
    setBusy(true);
    try {
      await writeValue(scope, field, value, describe);
    } finally {
      setBusy(false);
    }
  };

  const run = async (
    change: () => Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    >,
  ) => {
    setBusy(true);
    try {
      const result = await change();
      if (result.status === "error") {
        notify({ title: describe(result.error) });
      }
    } catch (error) {
      notify({ title: t("settings.endKeys.errors.save", { error }) });
    } finally {
      await refreshSettings();
      setBusy(false);
    }
  };

  return (
    <SettingsGroup
      title={t("settings.endKeys.title")}
      description={t("settings.endKeys.description")}
    >
      <ScopedSetting field="default_ends" summary={defaultEndsSummary}>
        {START_BINDINGS.map((binding) => (
          <DefaultEndSelect
            key={binding}
            binding={binding}
            value={defaultEndOf(binding, defaultEnds)}
            endKeys={endKeys}
            disabled={busy}
            onChange={(endId) =>
              scope
                ? save("default_ends", { ...defaultEnds, [binding]: endId })
                : run(() => commands.changeDefaultEndSetting(binding, endId))
            }
          />
        ))}
      </ScopedSetting>
      {isLinux && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.endKeys.linuxNoShortcuts")}
        </p>
      )}
      <ScopedSetting field="end_keys">
        <EndKeysEditor
          value={endKeys}
          defaultEnds={defaultEnds}
          disabled={busy}
          onChange={(next) =>
            scope
              ? save("end_keys", next)
              : run(() => commands.changeEndKeysSetting(next))
          }
        />
      </ScopedSetting>
    </SettingsGroup>
  );
};
