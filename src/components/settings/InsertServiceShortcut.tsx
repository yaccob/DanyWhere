import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { notify } from "@/stores/noticeStore";
import { formatKeyCombination } from "../../lib/utils/keyboard";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { useOsType } from "../../hooks/useOsType";
import { useShortcutCapture } from "../../hooks/useShortcutCapture";
import { usePressAway } from "../../hooks/usePressAway";

/** The shortcut DanyWhere gives the service at first: ⌃⌥⌘V. */
const DEFAULT_SHORTCUT = "ctrl+option+command+v";

interface InsertServiceShortcutProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * The shortcut of "Insert Last Dictation", the macOS service that inserts a
 * dictation without Accessibility. macOS keeps it in its services settings;
 * DanyWhere writes it there and reads it back whenever the window regains
 * focus, e.g. after the user changed it in System Settings.
 */
export const InsertServiceShortcut: React.FC<InsertServiceShortcutProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const osType = useOsType();
    const [keys, setKeys] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const shortcutRef = useRef<HTMLDivElement | null>(null);

    const refresh = useCallback(() => {
      commands
        .getInsertServiceShortcut()
        .then(setKeys)
        .catch(() => setKeys(null));
    }, []);

    useEffect(() => {
      refresh();
      window.addEventListener("focus", refresh);
      return () => window.removeEventListener("focus", refresh);
    }, [refresh]);

    const save = useCallback(
      async (binding: string) => {
        setSaving(true);
        const result = await commands.setInsertServiceShortcut(binding);
        setSaving(false);
        if (result.status === "ok") {
          setKeys(result.data);
          return;
        }
        const error = result.error;
        if (error === "invalid") {
          notify({ title: t("settings.general.insertService.invalid") });
        } else if (error.startsWith("conflict:")) {
          const id = error.slice("conflict:".length);
          notify({
            title: t("settings.general.insertService.conflict", {
              name: t(`settings.general.shortcut.bindings.${id}.name`, id),
            }),
          });
        } else {
          notify({
            title: t("settings.general.insertService.failed", {
              error: error.replace(/^failed:/, ""),
            }),
          });
        }
      },
      [t],
    );

    const { capturing, partial, start, cancel } = useShortcutCapture(
      "insert_service",
      osType,
      { save },
    );
    usePressAway(capturing, shortcutRef, cancel);

    const title = t("settings.general.insertService.title");
    return (
      <SettingContainer
        title={title}
        description={t("settings.general.insertService.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
        layout="horizontal"
      >
        <div className="flex items-center space-x-1">
          {capturing ? (
            <div
              ref={shortcutRef}
              className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/20 rounded-md"
            >
              {partial
                ? formatKeyCombination(partial, osType)
                : t("settings.general.shortcut.pressKeys")}
            </div>
          ) : (
            <div
              className="px-2 py-1 text-sm font-semibold bg-wash border border-line-strong hover:bg-logo-primary/10 rounded-md cursor-pointer hover:border-logo-primary"
              onClick={start}
            >
              {keys
                ? formatKeyCombination(keys, osType)
                : t("settings.general.insertService.none")}
            </div>
          )}
          <ResetButton
            ariaLabel={t("common.resetSetting", { setting: title })}
            onClick={() => save(DEFAULT_SHORTCUT)}
            disabled={saving}
          />
        </div>
      </SettingContainer>
    );
  });
