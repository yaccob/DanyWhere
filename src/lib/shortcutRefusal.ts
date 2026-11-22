import { endKeyErrorKey } from "./endKeys";

export type Translate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/**
 * DanyWhere: a refusal of the backend for a key combination, as a sentence:
 * the end shortcuts' own codes (end_keys.rs, also when a start shortcut's
 * change would leave a layer without a way to end), a combination the
 * service without Accessibility can't take, or another shortcut that
 * already has it ("conflict:<binding id>"), by that shortcut's name. `null`
 * for any other refusal, whose reason the caller shows as it came. One
 * place for the shortcut fields, the end shortcuts and the scoped writes
 * of a shortcut (qa 2026-11-22_2202, "Noch nicht geprüft": Linux).
 */
export const shortcutRefusalText = (
  error: string,
  t: Translate,
): string | null => {
  const key = endKeyErrorKey(error);
  if (key) return t(key);
  if (error.includes("service-shortcut-invalid")) {
    return t("settings.general.insertService.invalid");
  }
  const conflict = /conflict:([a-z_]+)/.exec(error);
  if (conflict) {
    return t("settings.general.insertService.conflict", {
      name: t(`settings.general.shortcut.bindings.${conflict[1]}.name`, {
        defaultValue: conflict[1],
      }),
    });
  }
  return null;
};
