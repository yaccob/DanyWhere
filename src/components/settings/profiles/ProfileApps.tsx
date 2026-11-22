import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import type { ApplicationInfo, Profile } from "@/bindings";
import {
  appChoices,
  appLabel,
  appsCaption,
  appsOf,
  ownerOfApp,
  withApp,
  withoutApp,
} from "@/lib/utils/profileApps";
import { Button } from "../../ui/Button";
import { Dropdown } from "../../ui/Dropdown";
import { Input } from "../../ui/Input";

interface ProfileAppsProps {
  profile: Profile;
  profiles: Profile[];
  /** The applications to offer, running ones first; empty where the system
   * doesn't list them (then an id is typed in). */
  applications: ApplicationInfo[];
  displayName: (profile: Profile) => string;
  busy: boolean;
  onChange: (apps: string[]) => void;
}

/** DanyWhere: the applications profile `profile` is used for automatically
 * (`app_profiles.rs`), under its row in the profile list. An application
 * assigned here leaves the profile it was assigned to before. */
export const ProfileApps: React.FC<ProfileAppsProps> = ({
  profile,
  profiles,
  applications,
  displayName,
  busy,
  onChange,
}) => {
  const { t } = useTranslation();
  const [typed, setTyped] = useState("");
  const assigned = appsOf(profile);
  const choices = appChoices(applications, assigned);

  const add = (appId: string) => {
    const apps = withApp(assigned, appId);
    if (apps !== assigned) onChange(apps);
  };

  return (
    <div className="flex flex-wrap items-center gap-2 px-4 pb-3 -mt-1 text-xs text-ink-2">
      <span className={assigned.length === 0 ? "text-ink-3" : undefined}>
        {t(appsCaption(assigned))}
      </span>
      {assigned.map((appId) => (
        <span
          key={appId}
          className="inline-flex items-center gap-1 rounded-full bg-wash border border-line px-2 py-0.5"
          title={appId}
        >
          {appLabel(appId, applications)}
          <button
            type="button"
            className="text-ink-3 hover:text-ink-2 disabled:opacity-50"
            aria-label={t("settings.profiles.apps.remove", {
              name: appLabel(appId, applications),
            })}
            disabled={busy}
            onClick={() => onChange(withoutApp(assigned, appId))}
          >
            <X className="w-3 h-3" />
          </button>
        </span>
      ))}
      {applications.length > 0 ? (
        <Dropdown
          compact
          selectedValue={null}
          placeholder={t("settings.profiles.apps.add")}
          disabled={busy || choices.length === 0}
          options={choices.map((app) => {
            const owner = ownerOfApp(profiles, app.id, profile.id);
            return {
              value: app.id,
              label: app.name,
              description: owner
                ? t("settings.profiles.apps.movesFrom", {
                    name: displayName(owner),
                  })
                : app.running
                  ? t("settings.profiles.apps.running")
                  : undefined,
            };
          })}
          onSelect={add}
        />
      ) : (
        <span className="inline-flex items-center gap-1">
          <Input
            variant="compact"
            value={typed}
            placeholder={t("settings.profiles.apps.idPlaceholder")}
            onChange={(event) => setTyped(event.target.value)}
            disabled={busy}
          />
          <Button
            variant="secondary"
            size="sm"
            disabled={busy || typed.trim() === ""}
            onClick={() => {
              add(typed);
              setTyped("");
            }}
          >
            {t("settings.profiles.apps.addTyped")}
          </Button>
        </span>
      )}
    </div>
  );
};
