// DanyWhere: settings for context from earlier dictations in the same window
// (`dictation_context.rs`, `window_capture.rs`).
import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AutomationApp,
  type AutomationStatus,
  type WindowTitleAccess,
} from "@/bindings";
import { SettingContainer, Textarea } from "@/components/ui";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { useSettings } from "../../../hooks/useSettings";
// DanyWhere: each row with its own profile wrapper (qa 2026-11-22_1603).
import { ScopedSetting } from "../profiles/scope/ScopedSetting";
import {
  LIMITS,
  filled,
  parseExcludedApps,
  parseLimit,
  titleAppGroups,
  type ContextSettings,
} from "./dictationContext";

type LimitKey = keyof typeof LIMITS;

/** The context settings of the scope around: what a row shows and how it
 * saves. Each row reads them itself, so under the defaults a deviating
 * profile's row (rendered in that profile's scope) shows that profile's. */
const useContextSettings = () => {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  return {
    settings: filled(getSetting("dictation_context")),
    save: (next: ContextSettings) => updateSetting("dictation_context", next),
    disabled: isUpdating("dictation_context"),
  };
};

/** A whole-number limit, saved when the field loses focus. */
const LimitField: React.FC<{
  field: LimitKey;
  title: string;
  description: string;
  unit?: string;
}> = ({ field, title, description, unit }) => {
  const { t } = useTranslation();
  const { settings, save, disabled } = useContextSettings();
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
          className={`w-20 text-end ${invalid ? "border-invalid" : ""}`}
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
 * Accessibility the apps DanyWhere may ask for theirs: the running ones, and
 * below them the installed ones that aren't running (macOS can only ask
 * about a running app). */
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
    // Back from starting an app or from System Settings: show what changed.
    const reload = () => void load();
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
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
  const { running, notRunning } = titleAppGroups(access.apps);

  const row = (app: AutomationApp) => (
    <div
      key={app.bundle_id}
      className="flex items-center justify-between gap-3"
    >
      <div className="min-w-0">
        <p className="text-sm font-medium">{app.name}</p>
        <p className="text-xs text-ink-3">{t(STATUS_KEYS[app.status])}</p>
      </div>
      {app.running && app.status === "not_asked" && (
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
  );

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
          {running.map(row)}
          {notRunning.length > 0 && (
            <>
              <p className="pt-2 text-xs font-medium text-ink-3">
                {t("settings.postProcessing.context.windowTitlesNotRunning")}
              </p>
              {notRunning.map(row)}
            </>
          )}
          <p className="text-xs text-ink-3">
            {t("settings.postProcessing.context.windowTitlesOtherApps")}
          </p>
        </div>
      )}
    </SettingContainer>
  );
};

/** The apps whose windows never give context, one per line. */
const ExcludedApps: React.FC = () => {
  const { t } = useTranslation();
  const { settings, save, disabled } = useContextSettings();
  const storedApps = settings.excluded_apps.join("\n");
  const [appsDraft, setAppsDraft] = useState(storedApps);
  useEffect(() => setAppsDraft(storedApps), [storedApps]);

  return (
    <SettingContainer
      title={t("settings.postProcessing.context.excludedApps")}
      description={t("settings.postProcessing.context.excludedAppsDescription")}
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
        disabled={disabled}
      />
    </SettingContainer>
  );
};

/** The limits, the exclusion list and the window titles. Each of the four
 * profile values is a row with its own wrapper; the privacy note and the
 * window titles (what this computer allows) are no profile values. */
export const DictationContextSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <>
      <div className="px-4 p-2">
        <p className="text-sm text-ink-3">
          {t("settings.postProcessing.context.privacy")}
        </p>
      </div>
      <ScopedSetting field="context_age_limit_minutes">
        <LimitField
          field="age_limit_minutes"
          title={t("settings.postProcessing.context.ageLimit")}
          description={t(
            "settings.postProcessing.context.ageLimitDescription",
            LIMITS.age_limit_minutes,
          )}
          unit={t("settings.postProcessing.context.minutes")}
        />
      </ScopedSetting>
      <ScopedSetting field="context_max_entries">
        <LimitField
          field="max_entries"
          title={t("settings.postProcessing.context.maxEntries")}
          description={t(
            "settings.postProcessing.context.maxEntriesDescription",
            LIMITS.max_entries,
          )}
        />
      </ScopedSetting>
      <ScopedSetting field="context_char_budget">
        <LimitField
          field="char_budget"
          title={t("settings.postProcessing.context.charBudget")}
          description={t(
            "settings.postProcessing.context.charBudgetDescription",
            LIMITS.char_budget,
          )}
        />
      </ScopedSetting>
      <ScopedSetting field="context_excluded_apps">
        <ExcludedApps />
      </ScopedSetting>
      <WindowTitles />
    </>
  );
};
