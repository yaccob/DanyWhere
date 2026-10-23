import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "../ui/Input";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { formatMsAsSeconds, parseSecondsToMs } from "@/lib/utils/seconds";

const MAX_SECONDS = 30;

interface ClipboardNoticeProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * How long the "on the clipboard" notice stays visible, typed in seconds
 * ("2.5" or "2,5"); 0 switches the notice off.
 */
export const ClipboardNoticeSetting: React.FC<ClipboardNoticeProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const storedMs = Number(getSetting("clipboard_notice_ms") ?? 2500);
    const formatted = formatMsAsSeconds(storedMs, i18n.language);
    const [draft, setDraft] = useState(formatted);
    const [invalid, setInvalid] = useState(false);

    // Follow changes from elsewhere (reset, other windows).
    useEffect(() => setDraft(formatted), [formatted]);

    const commit = () => {
      const ms = parseSecondsToMs(draft, 0, MAX_SECONDS);
      if (ms === null) {
        setInvalid(true);
        setDraft(formatted);
        return;
      }
      setInvalid(false);
      if (ms !== storedMs) updateSetting("clipboard_notice_ms", ms);
      setDraft(formatMsAsSeconds(ms, i18n.language));
    };

    return (
      <SettingContainer
        title={t("settings.advanced.clipboardNotice.title")}
        description={t("settings.advanced.clipboardNotice.description", {
          max: MAX_SECONDS,
        })}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center gap-2">
          <Input
            variant="compact"
            className={`w-20 text-end ${invalid ? "border-red-500" : ""}`}
            inputMode="decimal"
            value={draft}
            aria-invalid={invalid}
            title={
              invalid
                ? t("settings.advanced.clipboardNotice.invalid", {
                    max: MAX_SECONDS,
                  })
                : undefined
            }
            onChange={(e) => {
              setDraft(e.target.value);
              setInvalid(false);
            }}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
            }}
            disabled={isUpdating("clipboard_notice_ms")}
          />
          <span className="text-sm text-text/70">
            {t("settings.advanced.clipboardNotice.unit")}
          </span>
        </div>
      </SettingContainer>
    );
  });
