import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import {
  commands,
  type RecordingStorageLimit,
  type RecordingStorageUsage,
} from "@/bindings";
import {
  STORAGE_LIMITS,
  formatStorageSize,
  protectedExceedsLimit,
  storageLimitBytes,
} from "@/lib/utils/storageSize";

interface RecordingStorageLimitProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** How much space History's recordings may take; replaces the count and age options. */
export const RecordingStorageLimitSetting: React.FC<RecordingStorageLimitProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const limit: RecordingStorageLimit =
      getSetting("recording_storage_limit") ?? "mb_500";
    const [usage, setUsage] = useState<RecordingStorageUsage | null>(null);

    // Read again after a change: the new limit may have removed recordings.
    useEffect(() => {
      let cancelled = false;
      commands
        .getRecordingStorageUsage()
        .then((result) => {
          if (!cancelled && result.status === "ok") setUsage(result.data);
        })
        .catch((error) =>
          console.error("Failed to read recording storage usage:", error),
        );
      return () => {
        cancelled = true;
      };
    }, [limit]);

    const options = STORAGE_LIMITS.map((value) => {
      const bytes = storageLimitBytes(value);
      return {
        value,
        label:
          bytes === null
            ? t("settings.debug.recordingStorage.unlimited")
            : formatStorageSize(bytes, i18n.language),
      };
    });

    return (
      <SettingContainer
        title={t("settings.debug.recordingStorage.title")}
        description={t("settings.debug.recordingStorage.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex flex-col items-end gap-1">
          <Dropdown
            options={options}
            selectedValue={limit}
            onSelect={(value) =>
              updateSetting(
                "recording_storage_limit",
                value as RecordingStorageLimit,
              )
            }
            disabled={isUpdating("recording_storage_limit")}
          />
          {usage && (
            <p className="text-xs text-ink-3 text-right max-w-xs">
              {protectedExceedsLimit(usage, limit)
                ? t("settings.debug.recordingStorage.protectedOverLimit", {
                    size: formatStorageSize(
                      usage.protected_bytes,
                      i18n.language,
                    ),
                  })
                : t("settings.debug.recordingStorage.usage", {
                    size: formatStorageSize(usage.total_bytes, i18n.language),
                  })}
            </p>
          )}
        </div>
      </SettingContainer>
    );
  });

RecordingStorageLimitSetting.displayName = "RecordingStorageLimitSetting";
