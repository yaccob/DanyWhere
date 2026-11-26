import { keyRefusal, parseKeyRef, type KeyRef } from "./startKeys";

export type Translate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/** What a start, hold or end shortcut the backend refers to is called (a
 * start shortcut's keys, a hold or end shortcut's name); null when the
 * caller doesn't know it. */
export type KeyRefLabel = (ref: KeyRef) => string | null;

/** A sentence with a name left out reads without the gap. */
const tidy = (text: string): string =>
  text
    .replace(/\s+/g, " ")
    .replace(/\s+([,.:;!?])/g, "$1")
    .trim();

/** How the notice names a start, hold or end shortcut: "Start shortcut
 * ⌃ Space", "Hold shortcut ⌃⇧ Space held", "End shortcut Senden", or just
 * its kind when its name isn't known. */
const keyRefName = (ref: KeyRef, t: Translate, label?: KeyRefLabel): string => {
  const known = label?.(ref) ?? "";
  return tidy(
    ref.kind === "start"
      ? t("settings.startKeys.refName", { shortcut: known })
      : t(`settings.${ref.kind}Keys.refName`, { name: known }),
  );
};

/**
 * DanyWhere: a refusal of the backend for a key combination, as a sentence:
 * the start, hold and end shortcuts' own codes (qa 2026-11-26_1810,
 * "Prüfung und Garantien"), a combination the service without Accessibility
 * can't take, or another shortcut that already has it ("conflict:<binding
 * id>"), by that shortcut's name; a start, hold or end shortcut
 * (`conflict:start_key:<id>` …) by its keys or name through `label`. `null`
 * for any other refusal, whose reason the caller shows as it came. One
 * place for the shortcut fields, the three lists and the scoped writes of a
 * shortcut.
 */
export const shortcutRefusalText = (
  error: string,
  t: Translate,
  label?: KeyRefLabel,
): string | null => {
  const refusal = keyRefusal(error);
  if (refusal) {
    if (!refusal.ref) return t(refusal.key);
    return tidy(t(refusal.key, { shortcut: label?.(refusal.ref) ?? "" }));
  }
  if (error.includes("service-shortcut-invalid")) {
    return t("settings.general.insertService.invalid");
  }
  const conflict = /conflict:([a-z_]+)(?::([A-Za-z0-9_.-]+))?/.exec(error);
  if (conflict) {
    const ref = conflict[2]
      ? parseKeyRef(`${conflict[1]}:${conflict[2]}`)
      : null;
    const name = ref
      ? keyRefName(ref, t, label)
      : t(`settings.general.shortcut.bindings.${conflict[1]}.name`, {
          defaultValue: conflict[1],
        });
    return t("settings.general.insertService.conflict", { name });
  }
  return null;
};
