import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { Upload } from "lucide-react";
import {
  commands,
  type ImportPreview,
  type ImportResult,
  type KeyConflictChoice,
  type TransferError,
} from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "../../ui/Button";
import { ProfilePasswordDialog } from "./ProfilePasswordDialog";
import { transferErrorKey } from "@/lib/profileTransfer";
import { appLabel, droppedByOwner } from "@/lib/utils/profileApps";
import { profileDisplayName } from "@/lib/utils/profiles";
import { notify } from "@/stores/noticeStore";
import { useTransientStatus } from "./useTransientStatus";

interface PendingImport {
  path: string;
  preview: ImportPreview;
}

/**
 * Imports a profile from a YAML file as a new profile, without switching to
 * it. A file with an API key asks for its password; the profile can always
 * be imported without the key.
 */
export const ProfileImportButton: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [pending, setPending] = useState<PendingImport | null>(null);
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useTransientStatus();

  const providerName = (id: string | null | undefined) =>
    settings?.post_process_providers?.find((p) => p.id === id)?.label ??
    id ??
    "";

  const fail = (error: TransferError | unknown) => {
    const code =
      typeof error === "object" && error !== null && "code" in error
        ? (error as TransferError).code
        : "internal";
    notify({
      title: t("settings.profileTransfer.importFailed"),
      detail: t(transferErrorKey(code)),
    });
  };

  // DanyWhere: the file's applications another profile here already has
  // stay with that profile (`app_profiles`); say which, per profile.
  const reportDroppedApps = async (dropped: string[]) => {
    const groups = droppedByOwner(dropped, settings?.profiles ?? []);
    if (groups.length === 0) return;
    const applications = await commands.listApplications().catch(() => []);
    notify({
      title: t("settings.profileTransfer.droppedAppsTitle"),
      detail: groups
        .map(({ profile, apps }) =>
          t("settings.profileTransfer.droppedApps", {
            name: profileDisplayName(profile, t("tray.defaultProfile")),
            apps: apps.map((app) => appLabel(app, applications)).join(", "),
          }),
        )
        .join(" "),
    });
  };

  const finish = (result: ImportResult) => {
    setPending(null);
    setPasswordError(null);
    if (!result.profile) return;
    void reportDroppedApps(result.dropped_apps);
    const name = result.profile.name;
    setStatus(
      result.key === "added" || result.key === "replaced"
        ? t("settings.profileTransfer.importDoneWithKey", {
            name,
            provider: providerName(
              result.profile.values.post_process_provider_id,
            ),
          })
        : t("settings.profileTransfer.importDone", { name }),
    );
  };

  const run = async (
    path: string,
    password: string | null,
    choice: KeyConflictChoice,
  ) => {
    setBusy(true);
    try {
      const result = await commands.importProfile(path, password, choice);
      if (result.status === "error") {
        if (result.error.code === "wrong_password") {
          setPasswordError(t("settings.profileTransfer.errors.wrong_password"));
        } else {
          setPending(null);
          fail(result.error);
        }
        return;
      }
      if (result.data.conflict_provider_id) {
        const provider = providerName(result.data.conflict_provider_id);
        const replace = await ask(
          t("settings.profileTransfer.replaceKey", { provider }),
          {
            title: t("settings.profileTransfer.replaceKeyTitle"),
            kind: "warning",
            okLabel: t("settings.profileTransfer.replace"),
            cancelLabel: t("settings.profileTransfer.keep"),
          },
        );
        const again = await commands.importProfile(
          path,
          password,
          replace ? "replace" : "keep",
        );
        if (again.status === "ok") finish(again.data);
        else fail(again.error);
        return;
      }
      finish(result.data);
    } catch (error) {
      setPending(null);
      fail(error);
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: t("settings.profileTransfer.fileFilter"),
          extensions: ["yaml", "yml"],
        },
      ],
    });
    if (typeof path !== "string") return;
    const preview = await commands.inspectProfileImport(path);
    if (preview.status === "error") {
      fail(preview.error);
      return;
    }
    if (preview.data.has_api_key) {
      setPasswordError(null);
      setPending({ path, preview: preview.data });
    } else {
      await run(path, null, "ask");
    }
  };

  return (
    <span className="inline-flex items-center gap-2">
      <Button variant="secondary" size="sm" disabled={busy} onClick={start}>
        <span className="inline-flex items-center gap-1">
          <Upload className="h-3.5 w-3.5" aria-hidden="true" />
          {t("settings.profileTransfer.importButton")}
        </span>
      </Button>
      <ProfilePasswordDialog
        open={pending !== null}
        mode="import"
        profileName={pending?.preview.name ?? ""}
        providerName={providerName(pending?.preview.provider_id)}
        error={passwordError}
        busy={busy}
        onSubmit={(password) => pending && run(pending.path, password, "ask")}
        onWithoutKey={() => pending && run(pending.path, null, "ask")}
        onCancel={() => {
          setPending(null);
          setPasswordError(null);
        }}
      />
      {status && (
        <span role="status" className="text-xs text-ink-3">
          {status}
        </span>
      )}
    </span>
  );
};
