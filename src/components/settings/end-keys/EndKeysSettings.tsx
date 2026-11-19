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
import {
  START_BINDINGS,
  defaultEndOf,
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
 * default end of each start shortcut and the list. For now they edit the
 * active profile's values, which the flat settings hold; the editor and the
 * selections are controlled components, so they can later edit the
 * defaults or another profile instead.
 */
export const EndKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [busy, setBusy] = useState(false);
  const endKeys = getSetting("end_keys") ?? [];
  const defaultEnds = getSetting("default_ends") ?? {};
  const isLinux = type() === "linux";

  const run = async (
    change: () => Promise<
      { status: "ok"; data: null } | { status: "error"; error: string }
    >,
  ) => {
    setBusy(true);
    try {
      const result = await change();
      if (result.status === "error") {
        const key = endKeyErrorKey(result.error);
        const conflict = /conflict:([a-z_]+)/.exec(result.error);
        notify({
          title: key
            ? t(key)
            : conflict
              ? t("settings.general.insertService.conflict", {
                  name: t(
                    `settings.general.shortcut.bindings.${conflict[1]}.name`,
                    conflict[1],
                  ),
                })
              : t("settings.endKeys.errors.save", { error: result.error }),
        });
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
      {START_BINDINGS.map((binding) => (
        <DefaultEndSelect
          key={binding}
          binding={binding}
          value={defaultEndOf(binding, defaultEnds)}
          endKeys={endKeys}
          disabled={busy}
          onChange={(endId) =>
            run(() => commands.changeDefaultEndSetting(binding, endId))
          }
        />
      ))}
      {isLinux && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.endKeys.linuxNoShortcuts")}
        </p>
      )}
      <EndKeysEditor
        value={endKeys}
        defaultEnds={defaultEnds}
        disabled={busy}
        onChange={(next) => run(() => commands.changeEndKeysSetting(next))}
      />
    </SettingsGroup>
  );
};
