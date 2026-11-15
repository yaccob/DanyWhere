// DanyWhere: the frontend's rules for exporting and importing a profile.
import type { TransferErrorCode } from "@/bindings";

export type PasswordProblem = "empty" | "mismatch" | null;

/**
 * Whether a new export password can be used. There are no rules beyond "not
 * empty" and "typed twice alike": how strong it is, is the user's choice.
 */
export const checkNewPassword = (
  password: string,
  confirmation: string,
): PasswordProblem => {
  if (password.trim() === "") return "empty";
  if (password !== confirmation) return "mismatch";
  return null;
};

// Characters no file name may contain on Windows, macOS or Linux, and
// control characters.
const UNSAFE = /[/\\:*?"<>|\u0000-\u001f]+/g;

/** A file name for exporting the profile shown as `displayName`. */
export const exportFileName = (displayName: string): string => {
  const safe = displayName.replace(UNSAFE, "-").trim();
  const base = safe.replace(/^\.+$/, "") || "profile";
  return `${base}.yaml`;
};

/** The translation key of an export or import error. */
export const transferErrorKey = (code: TransferErrorCode): string =>
  `settings.profileTransfer.errors.${code}`;
