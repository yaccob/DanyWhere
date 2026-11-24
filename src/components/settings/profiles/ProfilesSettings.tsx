import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ApplicationInfo } from "@/bindings";
import { useProfiles } from "@/hooks/useProfiles";
import {
  DEFAULT_PROFILE_ID,
  canDeleteProfile,
  profileAfterDelete,
  type ProfileLike,
} from "@/lib/utils/profiles";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { SettingsGroup } from "../../ui/SettingsGroup";
// DanyWhere: the "?" to the manual's chapter on the defaults and profiles.
import { ManualHelpButton } from "../../ui/ManualHelpButton";
import { MANUAL_PAGES } from "@/lib/manual";
import { ProfileApps } from "./ProfileApps";
import { ProfileAutoByApp } from "./ProfileAutoByApp";
import { ProfileNameDialog } from "./ProfileNameDialog";
import { ProfileExportButton } from "./ProfileExportButton";
import { ProfileImportButton } from "./ProfileImportButton";

type Pending = {
  kind: "duplicate" | "rename" | "delete" | "create";
  profile: ProfileLike;
} | null;

/** The defaults as a row of the list: they are no profile, so they can be
 * used, exported and "duplicated" into a new profile that follows them (P9),
 * but not renamed, deleted or assigned applications. */
const DEFAULTS_ROW: ProfileLike = { id: DEFAULT_PROFILE_ID, name: "" };

/** DanyWhere: the profiles, managed on the page "Profiles": what is in use
 * (the defaults or a profile), and duplicate, rename or delete a profile. */
export const ProfilesSettings: React.FC = () => {
  const { t } = useTranslation();
  const {
    profiles,
    active,
    activeId,
    busy,
    defaultName,
    displayName,
    switchTo,
    duplicate,
    create,
    rename,
    remove,
    setApps,
  } = useProfiles();
  const [pending, setPending] = useState<Pending>(null);
  const close = () => setPending(null);
  // DanyWhere: the applications to assign (`app_profiles.rs`), from one
  // profile on, when assigning means anything.
  const [applications, setApplications] = useState<ApplicationInfo[]>([]);
  const several = profiles.length > 0;
  useEffect(() => {
    if (!several) return;
    commands
      .listApplications()
      .then(setApplications)
      .catch(() => setApplications([]));
  }, [several]);

  const naming =
    pending &&
    (pending.kind === "duplicate" ||
      pending.kind === "rename" ||
      pending.kind === "create")
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
        titleAction={
          <ManualHelpButton
            page={MANUAL_PAGES.defaultsAndProfiles}
            chapter={t("manual.chapters.defaultsAndProfiles")}
          />
        }
      >
        {[
          { profile: DEFAULTS_ROW, stored: null },
          ...profiles.map((stored) => ({ profile: stored, stored })),
        ].map(({ profile, stored }) => {
          const isDefaults = stored === null;
          const isActive = profile.id === activeId;
          const deletable = canDeleteProfile(profiles, profile.id);
          return (
            <div key={profile.id}>
              {/* The buttons go under the name when both don't fit, so a
                  narrow window doesn't cut the name off (qa 2026-11-22_0107,
                  A4). */}
              <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 min-h-12 px-4 p-2">
                <div className="flex min-w-0 flex-[1_1_10rem] items-center gap-2">
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
                <div className="ms-auto flex flex-wrap items-center justify-end gap-1">
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
                  {!isDefaults && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setPending({ kind: "rename", profile })}
                      disabled={busy}
                    >
                      {t("settings.profiles.rename")}
                    </Button>
                  )}
                  <ProfileExportButton
                    profileId={profile.id}
                    displayName={displayName(profile)}
                    disabled={busy}
                  />
                  {deletable && (
                    <Button
                      variant="danger-ghost"
                      size="sm"
                      onClick={() => setPending({ kind: "delete", profile })}
                      disabled={busy}
                    >
                      {t("settings.profiles.delete")}
                    </Button>
                  )}
                </div>
              </div>
              {several && stored && (
                <ProfileApps
                  profile={stored}
                  profiles={profiles}
                  applications={applications}
                  displayName={displayName}
                  busy={busy}
                  onChange={(apps) => void setApps(profile.id, apps)}
                />
              )}
            </div>
          );
        })}
        <ProfileAutoByApp />
        <div className="flex items-center justify-end gap-3 min-h-12 px-4 p-2">
          {/* DanyWhere (strand E2): a new profile follows the defaults in
              everything (P9). */}
          <Button
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={() =>
              setPending({ kind: "create", profile: DEFAULTS_ROW })
            }
          >
            {t("settings.profileView.overview.newProfile")}
          </Button>
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
              : naming.kind === "create"
                ? await create(name)
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
          {deleting && deleting.id === activeId && (
            <p>
              {t("settings.profiles.deleteDialog.switchesTo", {
                name: next ? displayName(next) : defaultName,
              })}
            </p>
          )}
        </div>
      </Dialog>
    </>
  );
};
