// DanyWhere: settings for context from earlier dictations in the same window
// (`dictation_context.rs`, `window_capture.rs`).
import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AutomationStatus,
  type WindowTitleAccess,
} from "@/bindings";
import { SettingContainer, Textarea } from "@/components/ui";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { useSettings } from "../../../hooks/useSettings";
import { ShortcutInput } from "../ShortcutInput";
import {
  LIMITS,
  filled,
  parseExcludedApps,
  parseLimit,
  type ContextSettings,
} from "./dictationContext";

type LimitKey = keyof typeof LIMITS;

/** A whole-number limit, saved when the field loses focus. */
const LimitField: React.FC<{
  field: LimitKey;
  title: string;
  description: string;
  unit?: string;
  settings: ContextSettings;
  save: (next: ContextSettings) => void;
  disabled: boolean;
}> = ({ field, title, description, unit, settings, save, disabled }) => {
  const { t } = useTranslation();
  const stored = settings[field];
  const [draft, setDraft] = useState(String(stored));
  const [invalid, setInvalid] = useState(false);
  const range = LIMITS[field];

  useEffect(() => setDraft(String(stored)), [stored]);

  const commit = () => {
    const value = parseLimit(draft, range);
    if (value === null) {
      setInvalid(true);
      setDraft(String(stored));
      return;
    }
    setInvalid(false);
    if (value !== stored) save({ ...settings, [field]: value });
  };

  return (
    <SettingContainer
      title={title}
      description={description}
      descriptionMode="tooltip"
      grouped={true}
    >
      <div className="flex items-center gap-2">
        <Input
          variant="compact"
          className={`w-20 text-end ${invalid ? "border-red-500" : ""}`}
          inputMode="numeric"
          value={draft}
          aria-invalid={invalid}
          title={
            invalid
              ? t("settings.postProcessing.context.invalid", range)
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
          disabled={disabled}
        />
        {unit && <span className="text-sm text-text/70">{unit}</span>}
      </div>
    </SettingContainer>
  );
};

const STATUS_KEYS: Record<AutomationStatus, string> = {
  allowed: "settings.postProcessing.context.statusAllowed",
  not_asked: "settings.postProcessing.context.statusNotAsked",
  denied: "settings.postProcessing.context.statusDenied",
  not_running: "settings.postProcessing.context.statusNotRunning",
  unknown: "settings.postProcessing.context.statusUnknown",
};

/** Where window titles come from on this computer, and on a Mac without
 * Accessibility the apps DanyWhere may ask for theirs. */
const WindowTitles: React.FC = () => {
  const { t } = useTranslation();
  const [access, setAccess] = useState<WindowTitleAccess | null>(null);
  const [asking, setAsking] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setAccess(await commands.getWindowTitleAccess());
    } catch (error) {
      console.error("Failed to read window title access:", error);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (!access) return null;

  const ask = async (bundleId: string) => {
    setAsking(bundleId);
    try {
      await commands.requestWindowTitleAccess(bundleId);
    } finally {
      setAsking(null);
      await load();
    }
  };

  const automation = access.supported && !access.accessibility;

  return (
    <SettingContainer
      title={t("settings.postProcessing.context.windowTitles")}
      description={
        !access.supported
          ? t("settings.postProcessing.context.windowTitlesUnsupported")
          : automation
            ? t("settings.postProcessing.context.windowTitlesAutomation")
            : t("settings.postProcessing.context.windowTitlesAvailable")
      }
      descriptionMode="inline"
      layout="stacked"
      grouped={true}
    >
      {automation && (
        <div className="space-y-2">
          {access.apps.map((app) => (
            <div
              key={app.bundle_id}
              className="flex items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <p className="text-sm font-medium">{app.name}</p>
                <p className="text-xs text-ink-3">
                  {t(STATUS_KEYS[app.status])}
                </p>
              </div>
              {app.status === "not_asked" && (
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => void ask(app.bundle_id)}
                  disabled={asking !== null}
                >
                  {t("settings.postProcessing.context.allow")}
                </Button>
              )}
            </div>
          ))}
          <p className="text-xs text-ink-3">
            {t("settings.postProcessing.context.windowTitlesOtherApps")}
          </p>
        </div>
      )}
    </SettingContainer>
  );
};

/** The shortcut that dictates with context, the limits, the exclusion list
 * and the window titles. */
export const DictationContextSettings: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const settings = filled(getSetting("dictation_context"));
  const updating = isUpdating("dictation_context");
  const save = (next: ContextSettings) =>
    updateSetting("dictation_context", next);

  const storedApps = settings.excluded_apps.join("\n");
  const [appsDraft, setAppsDraft] = useState(storedApps);
  useEffect(() => setAppsDraft(storedApps), [storedApps]);

  return (
    <>
      <ShortcutInput
        shortcutId="transcribe_with_context"
        descriptionMode="tooltip"
        grouped={true}
      />
      <div className="px-4 p-2">
        <p className="text-sm text-ink-3">
          {t("settings.postProcessing.context.privacy")}
        </p>
      </div>
      <LimitField
        field="age_limit_minutes"
        title={t("settings.postProcessing.context.ageLimit")}
        description={t(
          "settings.postProcessing.context.ageLimitDescription",
          LIMITS.age_limit_minutes,
        )}
        unit={t("settings.postProcessing.context.minutes")}
        settings={settings}
        save={save}
        disabled={updating}
      />
      <LimitField
        field="max_entries"
        title={t("settings.postProcessing.context.maxEntries")}
        description={t(
          "settings.postProcessing.context.maxEntriesDescription",
          LIMITS.max_entries,
        )}
        settings={settings}
        save={save}
        disabled={updating}
      />
      <LimitField
        field="char_budget"
        title={t("settings.postProcessing.context.charBudget")}
        description={t(
          "settings.postProcessing.context.charBudgetDescription",
          LIMITS.char_budget,
        )}
        settings={settings}
        save={save}
        disabled={updating}
      />
      <SettingContainer
        title={t("settings.postProcessing.context.excludedApps")}
        description={t(
          "settings.postProcessing.context.excludedAppsDescription",
        )}
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <Textarea
          variant="compact"
          className="w-full"
          value={appsDraft}
          onChange={(e) => setAppsDraft(e.target.value)}
          onBlur={() => {
            const apps = parseExcludedApps(appsDraft);
            if (apps.join("\n") !== storedApps) {
              save({ ...settings, excluded_apps: apps });
            }
          }}
          disabled={updating}
        />
      </SettingContainer>
      <WindowTitles />
    </>
  );
};
