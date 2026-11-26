import { useState } from "react";
import { useTranslation } from "react-i18next";
import { notify } from "@/stores/noticeStore";
import { useSettings } from "@/hooks/useSettings";
import { useSettingsScope } from "../profiles/scope/SettingsScope";
import { writeValue } from "../profiles/scope/profileWriter";
import { useDescribeEndKeyError } from "./describeEndKeyError";

type Outcome =
  | { status: "ok"; data: null }
  | { status: "error"; error: string };

/**
 * DanyWhere: writes a list of start, hold or end shortcuts (qa
 * 2026-11-26_1810): into the page's scope, the defaults or a profile, as
 * one value; outside a scope (the values in use) through the list's own
 * command (`change_start_keys_setting` …). A refusal reads as its sentence
 * (`useDescribeEndKeyError`), else as `saveKey` with the reason.
 */
export const useKeyListSave = <T>(
  field: "start_keys" | "hold_keys" | "end_keys",
  command: (list: T[]) => Promise<Outcome>,
  saveKey: string,
) => {
  const { t } = useTranslation();
  const { refreshSettings } = useSettings();
  const scope = useSettingsScope();
  const describe = useDescribeEndKeyError(saveKey);
  const [busy, setBusy] = useState(false);

  const save = async (next: T[]) => {
    setBusy(true);
    try {
      if (scope) {
        await writeValue(scope, field, next, describe);
        return;
      }
      const result = await command(next);
      if (result.status === "error") notify({ title: describe(result.error) });
    } catch (error) {
      notify({ title: t(saveKey, { error }) });
    } finally {
      // A scoped write refreshes by itself (`writeValue`).
      if (!scope) await refreshSettings();
      setBusy(false);
    }
  };

  return { busy, save };
};
