import React from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";
import {
  AboutSettings,
  AdvancedSettings,
  DebugSettings,
  GeneralSettings,
  HistorySettings,
  ModelsSettings,
  PostProcessingSettings,
} from "./settings";
import {
  AccessibilitySettings,
  ApplicationGeneralSettings,
  StorageSettings,
  UpdatesSettings,
} from "./settings/application";
import { ProfilesPage } from "./settings/profiles/ProfilesPage";
import { ProfilePagePlaceholder } from "./settings/profiles/ProfilePagePlaceholder";
import type { OnboardingStep } from "./onboarding/onboardingStore";
import { useProfiles } from "@/hooks/useProfiles";
import { navPath, selectArea, type NavLocation } from "@/lib/navigation";

interface SettingsPageProps {
  /** What to show, already resolved (`resolveLocation`). */
  location: NavLocation;
  onNavigate: (location: NavLocation) => void;
  onPreviewOnboarding: (step: OnboardingStep) => void;
  onRunOnboarding: () => void;
  onRunModelMigration: () => void;
}

/** DanyWhere: the path above a page ("Defaults › General"), since pages of
 * the same name are in several areas. Only for pages inside an area. */
const PagePath: React.FC<{ location: NavLocation }> = ({ location }) => {
  const { t } = useTranslation();
  const { profiles, displayName } = useProfiles();
  const path = navPath(location);
  if (path.length < 2) return null;
  return (
    <div className="max-w-3xl w-full mx-auto px-4 flex flex-none flex-wrap items-center gap-1 text-xs text-ink-3">
      {path.map((part, index) => {
        let text: string;
        if ("key" in part) {
          text = t(part.key);
        } else {
          const profile = profiles.find((p) => p.id === part.profileId);
          text = profile ? displayName(profile) : part.profileId;
        }
        return (
          <React.Fragment key={index}>
            {index > 0 && (
              <ChevronRight
                aria-hidden="true"
                className="size-3 shrink-0 rtl:rotate-180"
              />
            )}
            <span
              className={index === path.length - 1 ? "text-ink-2" : undefined}
            >
              {text}
            </span>
          </React.Fragment>
        );
      })}
    </div>
  );
};

/** DanyWhere: the page the navigation shows, in the three areas
 * (qa 2026-11-19_1711 P6, P11). */
export const SettingsPage: React.FC<SettingsPageProps> = ({
  location,
  onNavigate,
  onPreviewOnboarding,
  onRunOnboarding,
  onRunModelMigration,
}) => {
  let page: React.ReactNode;
  switch (location.kind) {
    case "defaults":
      switch (location.page) {
        case "general":
          page = <GeneralSettings />;
          break;
        case "models":
          page = (
            <ModelsSettings
              onChangeLanguages={() => onNavigate(selectArea("defaults"))}
            />
          );
          break;
        case "postprocessing":
          page = <PostProcessingSettings />;
          break;
        case "advanced":
          page = <AdvancedSettings />;
          break;
      }
      break;
    case "profiles":
      page = <ProfilesPage />;
      break;
    case "profile":
      page = (
        <ProfilePagePlaceholder
          profileId={location.profileId}
          page={location.page}
        />
      );
      break;
    case "application":
      switch (location.page) {
        case "general":
          page = <ApplicationGeneralSettings />;
          break;
        case "updates":
          page = <UpdatesSettings />;
          break;
        case "storage":
          page = <StorageSettings />;
          break;
        case "accessibility":
          page = <AccessibilitySettings />;
          break;
        case "debug":
          page = (
            <DebugSettings
              onPreviewOnboarding={onPreviewOnboarding}
              onRunOnboarding={onRunOnboarding}
              onRunModelMigration={onRunModelMigration}
            />
          );
          break;
      }
      break;
    case "history":
      page = <HistorySettings />;
      break;
    case "about":
      page = <AboutSettings />;
      break;
  }
  return (
    <>
      <PagePath location={location} />
      {page}
    </>
  );
};
