import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Profile } from "@/bindings";
import { useProfiles } from "@/hooks/useProfiles";
import { canDeleteProfile, profileAfterDelete } from "@/lib/utils/profiles";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ProfileNameDialog } from "./ProfileNameDialog";
import { ProfileExportButton } from "./ProfileExportButton";
import { ProfileImportButton } from "./ProfileImportButton";

type Pending = {
  kind: "duplicate" | "rename" | "delete";
  profile: Profile;
} | null;

/** DanyWhere: the profiles, managed at the top of General: which one is
 * active, and duplicate, rename or delete one. Hidden until the settings
 * carry profiles. */
export const ProfilesSettings: React.FC = () => {
  const { t } = useTranslation();
  const {
    profiles,
    active,
    busy,
    defaultName,
    displayName,
    switchTo,
    duplicate,
    rename,
    remove,
  } = useProfiles();
  const [pending, setPending] = useState<Pending>(null);
  const close = () => setPending(null);

  if (profiles.length === 0) return null;

  const naming =
    pending && (pending.kind === "duplicate" || pending.kind === "rename")
      ? { kind: pending.kind, profile: pending.profile }
      : null;
  const deleting = pending?.kind === "delete" ? pending.profile : null;
  const next = deleting
    ? profileAfterDelete(profiles, active?.id, deleting.id)
    : null;

  return (
    <>
      <SettingsGroup
        title={t("settings.profiles.title")}
        description={t("settings.profiles.description")}
      >
        {profiles.map((profile) => {
          const isActive = profile.id === active?.id;
          const deletable = canDeleteProfile(profiles, profile.id);
          return (
            <div
              key={profile.id}
              className="flex items-center justify-between gap-3 min-h-12 px-4 p-2"
            >
              <div className="flex items-center gap-2 min-w-0">
                <span
                  className="text-sm font-medium truncate"
                  title={displayName(profile)}
                >
                  {displayName(profile)}
                </span>
                {isActive && (
                  <span className="shrink-0 rounded-full bg-logo-primary/20 px-2 py-0.5 text-xs font-medium">
                    {t("settings.profiles.active")}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-1 shrink-0">
                {!isActive && (
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => switchTo(profile.id)}
                    disabled={busy}
                  >
                    {t("settings.profiles.use")}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setPending({ kind: "duplicate", profile })}
                  disabled={busy}
                >
                  {t("settings.profiles.duplicate")}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setPending({ kind: "rename", profile })}
                  disabled={busy}
                >
                  {t("settings.profiles.rename")}
                </Button>
                <ProfileExportButton
                  profileId={profile.id}
                  displayName={displayName(profile)}
                  disabled={busy}
                />
                <Button
                  variant="danger-ghost"
                  size="sm"
                  onClick={() => setPending({ kind: "delete", profile })}
                  disabled={busy || !deletable}
                  title={
                    deletable ? undefined : t("settings.profiles.lastProfile")
                  }
                >
                  {t("settings.profiles.delete")}
                </Button>
              </div>
            </div>
          );
        })}
        <div className="flex items-center justify-end gap-3 min-h-12 px-4 p-2">
          <ProfileImportButton />
        </div>
      </SettingsGroup>

      <ProfileNameDialog
        mode={naming?.kind ?? "rename"}
        profile={naming?.profile ?? null}
        profiles={profiles}
        defaultName={defaultName}
        displayName={displayName}
        busy={busy}
        onSubmit={async (name) => {
          if (!naming) return;
          const outcome =
            naming.kind === "duplicate"
              ? await duplicate(naming.profile.id, name)
              : await rename(naming.profile.id, name);
          if (outcome.ok) close();
        }}
        onClose={close}
      />

      <Dialog
        open={deleting !== null}
        title={t("settings.profiles.deleteDialog.title", {
          name: deleting ? displayName(deleting) : "",
        })}
        closeLabel={t("common.close")}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        contentFades={false}
        footer={
          <>
            <Button variant="secondary" size="md" onClick={close}>
              {t("settings.profiles.cancel")}
            </Button>
            <Button
              variant="danger"
              size="md"
              disabled={busy}
              onClick={async () => {
                if (!deleting) return;
                const outcome = await remove(deleting.id);
                if (outcome.ok) close();
              }}
            >
              {t("settings.profiles.deleteDialog.confirm")}
            </Button>
          </>
        }
      >
        <div className="space-y-2 text-sm">
          <p>{t("settings.profiles.deleteDialog.description")}</p>
          {deleting && deleting.id === active?.id && next && (
            <p>
              {t("settings.profiles.deleteDialog.switchesTo", {
                name: displayName(next),
              })}
            </p>
          )}
        </div>
      </Dialog>
    </>
  );
};
