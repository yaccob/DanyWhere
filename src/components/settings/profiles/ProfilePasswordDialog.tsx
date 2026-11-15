import React, {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Input } from "../../ui/Input";
import { checkNewPassword } from "@/lib/profileTransfer";

interface ProfilePasswordDialogProps {
  open: boolean;
  /** Export asks for a new password twice; import asks for it once. */
  mode: "export" | "import";
  /** The profile's shown name. */
  profileName: string;
  /** Import only: the provider the file's key belongs to. */
  providerName?: string;
  /** A message under the field, e.g. a wrong password. */
  error?: string | null;
  /** While the key is encrypted or decrypted (about a second). */
  busy?: boolean;
  onSubmit: (password: string) => void;
  /** Import only: import the profile without its key. */
  onWithoutKey?: () => void;
  onCancel: () => void;
}

/** Asks for the password that protects a profile's API key in its file. */
export const ProfilePasswordDialog: React.FC<ProfilePasswordDialogProps> = ({
  open,
  mode,
  profileName,
  providerName,
  error,
  busy = false,
  onSubmit,
  onWithoutKey,
  onCancel,
}) => {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const passwordId = useId();
  // Input doesn't forward refs; the dialog focuses this element when it opens.
  const passwordRef = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    passwordRef.current = open ? document.getElementById(passwordId) : null;
  }, [open, passwordId]);

  // A new dialog starts empty; nothing typed stays around.
  useEffect(() => {
    if (!open) {
      setPassword("");
      setConfirmation("");
      setProblem(null);
    }
  }, [open]);

  const submit = (event?: React.FormEvent) => {
    event?.preventDefault();
    if (busy) return;
    if (mode === "export") {
      const check = checkNewPassword(password, confirmation);
      if (check === "empty") {
        setProblem(t("settings.profileTransfer.errors.empty_password"));
        return;
      }
      if (check === "mismatch") {
        setProblem(t("settings.profileTransfer.passwordMismatch"));
        return;
      }
    } else if (password === "") {
      setProblem(t("settings.profileTransfer.errors.empty_password"));
      return;
    }
    setProblem(null);
    onSubmit(password);
  };

  const shownError = problem ?? error ?? null;
  const formId = `profile-password-${mode}`;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onCancel();
      }}
      dismissible={!busy}
      closeLabel={t("common.close")}
      initialFocusRef={passwordRef as React.RefObject<HTMLElement>}
      title={
        mode === "export"
          ? t("settings.profileTransfer.exportTitle", { name: profileName })
          : t("settings.profileTransfer.importTitle", { name: profileName })
      }
      description={
        mode === "export"
          ? t("settings.profileTransfer.exportDescription")
          : t("settings.profileTransfer.importDescription", {
              name: profileName,
              provider: providerName ?? "",
            })
      }
      footer={
        <>
          {mode === "import" && onWithoutKey && (
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={onWithoutKey}
            >
              {t("settings.profileTransfer.withoutKey")}
            </Button>
          )}
          <Button
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={onCancel}
          >
            {t("settings.profileTransfer.cancel")}
          </Button>
          <Button
            variant="primary"
            size="sm"
            type="submit"
            form={formId}
            disabled={busy}
          >
            {busy
              ? t("settings.profileTransfer.working")
              : mode === "export"
                ? t("settings.profileTransfer.export")
                : t("settings.profileTransfer.import")}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="flex flex-col gap-3">
        <label className="flex flex-col gap-1 text-sm">
          <span>{t("settings.profileTransfer.password")}</span>
          <Input
            id={passwordId}
            type="password"
            autoComplete="new-password"
            value={password}
            disabled={busy}
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        {mode === "export" && (
          <label className="flex flex-col gap-1 text-sm">
            <span>{t("settings.profileTransfer.confirmPassword")}</span>
            <Input
              type="password"
              autoComplete="new-password"
              value={confirmation}
              disabled={busy}
              onChange={(event) => setConfirmation(event.target.value)}
            />
          </label>
        )}
        {shownError && (
          <p role="alert" className="text-sm text-error">
            {shownError}
          </p>
        )}
      </form>
    </Dialog>
  );
};
