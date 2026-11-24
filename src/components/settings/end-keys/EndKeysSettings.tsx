import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { commands } from "@/bindings";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "@/hooks/useSettings";
import { postProcessingOn } from "@/lib/endKeys";
import { SettingsGroup } from "../../ui/SettingsGroup";
// DanyWhere: the "?" to the manual's chapter on start and end shortcuts.
import { ManualHelpButton } from "../../ui/ManualHelpButton";
import { MANUAL_PAGES } from "@/hooks/useOpenManual";
import { EndKeysEditor } from "./EndKeysEditor";
// DanyWhere (strand E2): the end shortcuts of the page's scope, the defaults
// or a profile, each with its chain.
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import { useSettingsScope } from "../profiles/scope/SettingsScope";
import { writeValue } from "../profiles/scope/profileWriter";
import { useDescribeEndKeyError } from "./describeEndKeyError";

/**
 * DanyWhere: the end shortcuts on the shortcut settings (end_keys.rs, qa
 * 2026-11-22_2134): the list of the page's scope (the defaults or a
 * profile; outside one, the values in use). How a held start shortcut ends
 * is "When Held", under "Shortcut Behavior" (`HoldEndsSetting`).
 */
export const EndKeysSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, refreshSettings } = useSettings();
  const [busy, setBusy] = useState(false);
  const endKeys = getSetting("end_keys") ?? [];
  const bindings = getSetting("bindings") ?? {};
  // The scope's own switch: the defaults' or the profile's value.
  const postProcessingOff = !postProcessingOn({
    post_process_enabled: getSetting("post_process_enabled"),
  });
  const isLinux = type() === "linux";
  const scope = useSettingsScope();
  const describe = useDescribeEndKeyError();

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
      titleAction={
        <ManualHelpButton
          page={MANUAL_PAGES.startAndEndShortcuts}
          chapter={t("manual.chapters.startAndEndShortcuts")}
        />
      }
    >
      {isLinux && (
        <p className="px-4 py-2 text-xs text-ink-3">
          {t("settings.endKeys.linuxNoShortcuts")}
        </p>
      )}
      <ScopedSetting field="end_keys">
        <EndKeysEditor
          value={endKeys}
          bindings={bindings}
          disabled={busy}
          postProcessingOff={postProcessingOff}
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
