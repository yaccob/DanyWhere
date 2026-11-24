import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type HoldEnd } from "@/bindings";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import {
  START_BINDINGS,
  holdEndOf,
  postProcessingOn,
  secondStartTaken,
  type TakenFrom,
} from "@/lib/endKeys";
import { useSettingsStore } from "@/stores/settingsStore";
import { SettingContainer } from "../../ui/SettingContainer";
import { AppendTextInput } from "../AppendText";
import { useSettingsScope } from "../profiles/scope/SettingsScope";
import { writeValue } from "../profiles/scope/profileWriter";
import { useDescribeEndKeyError } from "./describeEndKeyError";

/**
 * DanyWhere: "When Held" (end_keys.rs, qa 2026-11-22_2134): how releasing
 * a held start shortcut ends the recording, per start shortcut, with
 * "Append", post-processing and context, but without a name or a
 * combination of its own. One profile value as a whole (`hold_ends`), of
 * the page's scope (the defaults or a profile; outside one, the values in
 * use). Shown only where a start shortcut can be held, first under "End
 * Shortcuts" (user, 2026-11-24).
 */
export const HoldEndsSetting: React.FC = () => {
  const { t } = useTranslation();
  const osType = useOsType();
  const { getSetting, refreshSettings } = useSettings();
  const scope = useSettingsScope();
  const describe = useDescribeEndKeyError();
  const [busy, setBusy] = useState(false);
  const holdEnds = getSetting("hold_ends") ?? {};
  const bindings = getSetting("bindings") ?? {};
  // The second start shortcut only while DanyWhere takes it, which the
  // settings in use decide, not the page's scope.
  const inUse = useSettingsStore((state) => state.settings);
  const shown = START_BINDINGS.filter(
    (binding) =>
      binding !== "transcribe_with_post_process" ||
      secondStartTaken((inUse ?? {}) as TakenFrom),
  );
  // Whether the page's scope post-processes at all: with its switch off no
  // held end does (user, 2026-11-23, option a).
  const postProcessingOff = !postProcessingOn({
    post_process_enabled: getSetting("post_process_enabled"),
  });

  /** Changes the end of `binding`; both are written, as one value. */
  const change = async (binding: string, part: Partial<HoldEnd>) => {
    const next: Partial<Record<string, HoldEnd>> = Object.fromEntries(
      START_BINDINGS.map((id) => [id, holdEndOf(id, holdEnds)]),
    );
    next[binding] = { ...holdEndOf(binding, holdEnds), ...part };
    setBusy(true);
    try {
      if (scope) {
        await writeValue(scope, "hold_ends", next, describe);
        return;
      }
      const result = await commands.changeHoldEndsSetting(next);
      if (result.status === "error") notify({ title: describe(result.error) });
      await refreshSettings();
    } catch (error) {
      notify({ title: t("settings.endKeys.errors.save", { error }) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingContainer
      title={t("settings.endKeys.holdEnds.title")}
      description={t("settings.endKeys.holdEnds.description")}
      descriptionMode="tooltip"
      grouped={true}
      layout="stacked"
    >
      <div className="flex flex-col gap-3">
        {shown.map((binding) => {
          const end = holdEndOf(binding, holdEnds);
          const keys = bindings[binding]?.current_binding ?? "";
          const shortcut = keys
            ? formatKeyCombination(keys, osType, t)
            : t(`settings.general.shortcut.bindings.${binding}.name`);
          return (
            <div key={binding} className="flex flex-col gap-2">
              <span className="text-sm font-semibold">
                {t("settings.endKeys.held", { shortcut })}
              </span>
              <div className="flex flex-col gap-1">
                <span
                  className="text-xs text-ink-3"
                  title={t("settings.endKeys.appendDescription")}
                >
                  {t("settings.endKeys.append")}
                </span>
                <AppendTextInput
                  value={end.append_text}
                  onChange={(append_text) => change(binding, { append_text })}
                  disabled={busy}
                />
              </div>
              <div className="flex flex-wrap items-center gap-4 text-sm">
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    className="accent-logo-primary"
                    checked={end.post_process}
                    disabled={busy}
                    onChange={(event) =>
                      void change(binding, {
                        post_process: event.target.checked,
                      })
                    }
                  />
                  {t("settings.endKeys.postProcess")}
                </label>
                <label
                  className="flex items-center gap-2 cursor-pointer"
                  title={t("settings.endKeys.sendContextHint")}
                >
                  <input
                    type="checkbox"
                    className="accent-logo-primary"
                    checked={end.send_context}
                    disabled={busy || !end.post_process}
                    onChange={(event) =>
                      void change(binding, {
                        send_context: event.target.checked,
                      })
                    }
                  />
                  {t("settings.endKeys.sendContext")}
                </label>
              </div>
              {end.post_process && postProcessingOff && (
                <p className="text-xs text-ink-3">
                  {t("settings.endKeys.postProcessOff")}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </SettingContainer>
  );
};
