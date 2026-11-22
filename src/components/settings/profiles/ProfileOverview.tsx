// DanyWhere: the page "Profile" of a profile (qa 2026-11-19_1711 sketch 4,
// P5, P8, P9): its name and applications, all its own values in one place
// (each opens its page and has its own reset), "Whole Profile to Defaults"
// (a question naming the values; name and applications stay), duplicate,
// export, delete.
import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link2Off, RotateCcw } from "lucide-react";
import { commands, type ApplicationInfo } from "@/bindings";
import { useProfiles } from "@/hooks/useProfiles";
import { useSettingsStore } from "@/stores/settingsStore";
import { pageLabelKey } from "@/lib/navigation";
import { pageOfValue } from "@/lib/profileFields";
import { jumpTarget } from "@/lib/settingJump";
import {
  ownValues,
  type LayeredProfile,
  type OwnValue,
} from "@/lib/profileScope";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { ProfileApps } from "./ProfileApps";
import { ProfileExportButton } from "./ProfileExportButton";
import { ProfileNameDialog } from "./ProfileNameDialog";
import { runCoupling } from "./scope/profileWriter";
import { ResetDialog } from "./scope/ResetDialog";
import { useOpenProfilePage } from "./scope/SettingsScope";
import { useValueLabels } from "./scope/useValueLabels";

type Pending = "rename" | "duplicate" | "delete" | "reset" | null;

export const ProfileOverview: React.FC<{ profileId: string }> = ({
  profileId,
}) => {
  const { t } = useTranslation();
  const {
    profiles,
    activeId,
    busy,
    defaultName,
    displayName,
    duplicate,
    rename,
    remove,
    setApps,
  } = useProfiles();
  const layered = useSettingsStore(
    (s) => (s.settings?.profiles ?? []) as unknown as readonly LayeredProfile[],
  );
  const openPage = useOpenProfilePage();
  const { ownValueLabel, ownValueRow } = useValueLabels();
  const [pending, setPending] = useState<Pending>(null);
  const [applications, setApplications] = useState<ApplicationInfo[]>([]);

  useEffect(() => {
    commands
      .listApplications()
      .then(setApplications)
      .catch(() => setApplications([]));
  }, []);

  const profile = profiles.find((p) => p.id === profileId);
  const stored = layered.find((p) => p.id === profileId);
  if (!profile || !stored) return null;
  const name = displayName(profile);
  const own = ownValues(stored);
  const close = () => setPending(null);

  /** "Setting: value" for an own value; a switched-off entry says so. */
  const rowText = (value: OwnValue): string =>
    ownValueRow(value, stored.values);
  const nameOf = (value: OwnValue): string =>
    ownValueLabel(value, stored.values);

  const resetOne = (value: OwnValue) =>
    void runCoupling(() =>
      value.entry === undefined
        ? commands.resetProfileValues(profileId, [value.field])
        : commands.resetProfileEntry(profileId, value.field, value.entry),
    );

  const resetWhole = async () => {
    await runCoupling(() => commands.resetWholeProfile(profileId));
    close();
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup>
        <div className="flex items-center justify-between gap-3 min-h-12 px-4 p-2">
          <div className="flex min-w-0 items-center gap-3">
            <span className="text-sm text-ink-3">
              {t("settings.profiles.nameLabel")}
            </span>
            <span className="truncate text-sm font-semibold" title={name}>
              {name}
            </span>
            {profile.id === activeId && (
              <span className="shrink-0 rounded-full bg-logo-primary/20 px-2 py-0.5 text-xs font-medium">
                {t("settings.profiles.active")}
              </span>
            )}
          </div>
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => setPending("rename")}
          >
            {t("settings.profiles.rename")}
          </Button>
        </div>
        <ProfileApps
          profile={profile}
          profiles={profiles}
          applications={applications}
          displayName={displayName}
          busy={busy}
          onChange={(apps) => void setApps(profile.id, apps)}
        />
      </SettingsGroup>

      <SettingsGroup
        title={t("settings.profileView.overview.ownValues", {
          number: own.length,
        })}
        description={
          own.length > 0
            ? t("settings.profileView.overview.jumpHint")
            : t("settings.profileView.overview.none")
        }
      >
        {own.map((value) => {
          const page = pageOfValue(value.field, value.entry);
          return (
            <div
              key={`${value.field}/${value.entry ?? ""}/${value.off ? "off" : "own"}`}
              className="flex items-center gap-2 px-4 py-1.5"
            >
              <button
                type="button"
                // Opens the page and jumps to the setting there (B5).
                onClick={() =>
                  openPage?.(profileId, page, jumpTarget(profileId, value))
                }
                className="flex min-w-0 flex-1 items-center gap-2 text-start text-sm cursor-pointer hover:text-logo-primary"
              >
                <span className="w-28 shrink-0 truncate text-xs text-ink-3">
                  {t(pageLabelKey(page))}
                </span>
                <Link2Off
                  className="size-3.5 shrink-0 text-logo-primary"
                  aria-hidden="true"
                />
                <span className="min-w-0 truncate">{rowText(value)}</span>
              </button>
              <button
                type="button"
                onClick={() => resetOne(value)}
                title={t("settings.profileView.resetTooltip")}
                aria-label={t("settings.profileView.resetEntry", {
                  name: nameOf(value),
                })}
                className="shrink-0 rounded p-1 text-ink-3 hover:text-logo-primary hover:bg-logo-primary/10 cursor-pointer"
              >
                <RotateCcw className="size-3.5" aria-hidden="true" />
              </button>
            </div>
          );
        })}
      </SettingsGroup>

      <div className="flex flex-wrap items-center gap-2 px-4">
        <Button
          variant="secondary"
          size="sm"
          disabled={busy || own.length === 0}
          onClick={() => setPending("reset")}
        >
          <span className="inline-flex items-center gap-1">
            <RotateCcw className="size-3.5" aria-hidden="true" />
            {t("settings.profileView.overview.resetWhole")}
          </span>
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={busy}
          onClick={() => setPending("duplicate")}
        >
          {t("settings.profiles.duplicate")}
        </Button>
        <ProfileExportButton
          profileId={profile.id}
          displayName={name}
          disabled={busy}
        />
        <Button
          variant="danger-ghost"
          size="sm"
          disabled={busy}
          onClick={() => setPending("delete")}
        >
          {t("settings.profiles.delete")}
        </Button>
      </div>

      <ProfileNameDialog
        mode={pending === "duplicate" ? "duplicate" : "rename"}
        profile={
          pending === "rename" || pending === "duplicate" ? profile : null
        }
        profiles={profiles}
        defaultName={defaultName}
        displayName={displayName}
        busy={busy}
        onSubmit={async (next) => {
          const outcome =
            pending === "duplicate"
              ? await duplicate(profile.id, next)
              : await rename(profile.id, next);
          if (outcome.ok) close();
        }}
        onClose={close}
      />

      <ResetDialog
        open={pending === "reset"}
        title={t("settings.profileView.overview.resetWholeTitle", { name })}
        values={own.map(nameOf)}
        note={t("settings.profileView.overview.keeps")}
        busy={busy}
        onConfirm={() => void resetWhole()}
        onClose={close}
      />

      <Dialog
        open={pending === "delete"}
        title={t("settings.profiles.deleteDialog.title", { name })}
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
                const outcome = await remove(profile.id);
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
          {profile.id === activeId && (
            <p>
              {t("settings.profiles.deleteDialog.switchesTo", {
                name: defaultName,
              })}
            </p>
          )}
        </div>
      </Dialog>
    </div>
  );
};
