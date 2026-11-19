import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { checkProfileName, type ProfileLike } from "@/lib/utils/profiles";
import { isComposingKeyEvent } from "@/lib/utils/keyboard";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";

interface ProfileNameDialogProps {
  /** Duplicate `profile` under a new name (from the defaults: a new
   * profile that follows them), or rename it. */
  mode: "duplicate" | "rename";
  /** The profile the dialog is for; `null` keeps it closed. */
  profile: ProfileLike | null;
  profiles: ProfileLike[];
  defaultName: string;
  displayName: (profile: ProfileLike) => string;
  busy: boolean;
  onSubmit: (name: string) => Promise<void>;
  onClose: () => void;
}

/** DanyWhere: asks for a profile's name, for a duplicate or a rename. */
export const ProfileNameDialog: React.FC<ProfileNameDialogProps> = ({
  mode,
  profile,
  profiles,
  defaultName,
  displayName,
  busy,
  onSubmit,
  onClose,
}) => {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState("");

  // A rename starts from the stored name, a duplicate from nothing.
  useEffect(() => {
    if (profile) setDraft(mode === "rename" ? profile.name : "");
  }, [mode, profile]);

  if (!profile) return null;

  const check = checkProfileName(
    profiles,
    draft,
    defaultName,
    mode === "rename" ? profile : undefined,
  );
  const canSubmit = check.kind === "ok" && !busy;
  const submit = () => {
    if (check.kind === "ok" && !busy) void onSubmit(check.name);
  };

  const name = displayName(profile);
  const keys =
    mode === "duplicate"
      ? "settings.profiles.duplicateDialog"
      : "settings.profiles.renameDialog";

  return (
    <Dialog
      open
      title={t(`${keys}.title`, { name })}
      description={
        mode === "duplicate"
          ? t("settings.profiles.duplicateDialog.description")
          : undefined
      }
      closeLabel={t("common.close")}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      initialFocusRef={inputRef as React.RefObject<HTMLElement>}
      contentFades={false}
      footer={
        <>
          <Button variant="secondary" size="md" onClick={onClose}>
            {t("settings.profiles.cancel")}
          </Button>
          <Button
            variant="primary"
            size="md"
            onClick={submit}
            disabled={!canSubmit}
          >
            {t(`${keys}.confirm`)}
          </Button>
        </>
      }
    >
      <div className="space-y-2">
        <label className="block text-sm font-semibold">
          {t("settings.profiles.nameLabel")}
          {/* A plain input, styled as Input: Input passes no ref on, and
              the dialog focuses the field through one. */}
          <input
            ref={inputRef}
            type="text"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (isComposingKeyEvent(e.nativeEvent)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                submit();
              }
            }}
            placeholder={t("settings.profiles.namePlaceholder")}
            className="mt-1 block w-full px-2 py-1 text-sm font-semibold bg-wash border border-line-strong rounded-md text-start transition-all duration-150 hover:bg-logo-primary/10 hover:border-logo-primary focus:outline-none focus:bg-logo-primary/20 focus:border-logo-primary"
          />
        </label>
        {check.kind === "taken" && (
          <p className="text-xs text-error">
            {t("settings.profiles.nameTaken")}
          </p>
        )}
      </div>
    </Dialog>
  );
};
