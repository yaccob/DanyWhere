import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";
import { commands, type TransferError } from "@/bindings";
import { Button } from "../../ui/Button";
import { ProfilePasswordDialog } from "./ProfilePasswordDialog";
import { exportFileName, transferErrorKey } from "@/lib/profileTransfer";
import { notify } from "@/stores/noticeStore";
import { useTransientStatus } from "./useTransientStatus";

interface ProfileExportButtonProps {
  profileId: string;
  /** The profile's shown name, also the suggested file name. */
  displayName: string;
  /** E.g. while the profile list is busy with another change. */
  disabled?: boolean;
}

/**
 * Exports one profile to a YAML file. A profile that carries an API key asks
 * for a password first, which encrypts the key in the file.
 */
export const ProfileExportButton: React.FC<ProfileExportButtonProps> = ({
  profileId,
  displayName,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const [askPassword, setAskPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useTransientStatus();

  const fail = (error: TransferError | unknown) => {
    const code =
      typeof error === "object" && error !== null && "code" in error
        ? (error as TransferError).code
        : "internal";
    notify({
      title: t("settings.profileTransfer.exportFailed"),
      detail: t(transferErrorKey(code)),
    });
  };

  const exportTo = async (password: string | null) => {
    const path = await save({
      defaultPath: exportFileName(displayName),
      filters: [
        {
          name: t("settings.profileTransfer.fileFilter"),
          extensions: ["yaml", "yml"],
        },
      ],
    });
    if (!path) return;
    setBusy(true);
    try {
      const result = await commands.exportProfile(profileId, path, password);
      if (result.status === "ok") {
        setAskPassword(false);
        setStatus(t("settings.profileTransfer.exportDone"));
      } else {
        fail(result.error);
      }
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    const needs = await commands.profileExportNeedsPassword(profileId);
    if (needs.status === "error") {
      fail(needs.error);
      return;
    }
    if (needs.data) {
      setAskPassword(true);
    } else {
      await exportTo(null);
    }
  };

  return (
    <span className="inline-flex items-center gap-2">
      <Button
        variant="ghost"
        size="sm"
        disabled={busy || disabled}
        onClick={start}
        title={t("settings.profileTransfer.exportTitle", { name: displayName })}
      >
        {t("settings.profileTransfer.export")}
      </Button>
      <ProfilePasswordDialog
        open={askPassword}
        mode="export"
        profileName={displayName}
        busy={busy}
        onSubmit={(password) => exportTo(password)}
        onCancel={() => setAskPassword(false)}
      />
      {status && (
        <span role="status" className="text-xs text-ink-3">
          {status}
        </span>
      )}
    </span>
  );
};
