import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { DoneMark } from "../../shared/DoneMark";
import { useAccessibility } from "../../../hooks/useAccessibility";
import { useInputMethod } from "../../../hooks/useInputMethod";
import { useOsType } from "../../../hooks/useOsType";
import { offersInputMethod } from "@/lib/inputMethod";

/**
 * DanyWhere: Application Settings › Accessibility, "Insert Without the
 * Clipboard": DanyWhere's own input method, macOS's way of inserting once the
 * user has added its input source in System Settings (PROTOCOL.md, "The
 * rule"). Without Accessibility it is how end shortcuts insert at once; with
 * it, an offer. Only on macOS in a build that carries the input method. The
 * state follows System Settings while the page is open.
 */
export const InputMethodSettings: React.FC = () => {
  const { t } = useTranslation();
  const os = useOsType();
  const { runsWithoutAccessibility } = useAccessibility();
  const { status, name, settingUp, setUp } = useInputMethod({
    enabled: os === "macos",
  });
  if (!offersInputMethod(os, status)) return null;

  return (
    <SettingsGroup title={t("settings.inputMethod.title")}>
      <div className="space-y-2 px-4 py-3 text-sm">
        {status?.added ? (
          <>
            <p className="flex items-center gap-2 font-medium">
              <DoneMark />
              {t("settings.inputMethod.added")}
            </p>
            <p className="text-ink-2">
              {t("settings.inputMethod.stop", { name })}
            </p>
          </>
        ) : (
          <>
            <p>{t("settings.inputMethod.description")}</p>
            <p className="text-ink-2">{t("settings.inputMethod.howItWorks")}</p>
            <p className="text-ink-2">
              {runsWithoutAccessibility
                ? t("settings.inputMethod.withoutAccessibility", {
                    shortcut: t(
                      "settings.general.shortcut.bindings.paste_last_transcript.name",
                    ),
                  })
                : t("settings.inputMethod.offer")}
            </p>
            <p>{t("settings.inputMethod.steps", { name })}</p>
            <div className="pt-1">
              <Button
                variant="secondary"
                size="sm"
                disabled={settingUp}
                onClick={setUp}
              >
                {t("settings.inputMethod.setUp")}
              </Button>
            </div>
          </>
        )}
      </div>
    </SettingsGroup>
  );
};
