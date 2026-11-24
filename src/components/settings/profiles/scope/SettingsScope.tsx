// DanyWhere: which values a settings page reads and writes (qa 2026-11-19_1711
// P4, P7): the defaults ("Grundeinstellungen") or one profile. Every page of
// the defaults and of a profile renders inside a scope; `useSettings` reads
// it, so Handy's setting components show and change the scope's values with
// no code of their own. Outside a scope (onboarding, History, the overlay)
// everything works as before, on the values in use.
import React, { createContext, useContext } from "react";
import type { Scope } from "@/lib/profileScope";
import type { JumpTarget } from "@/lib/settingJump";
import type { NavLocation } from "@/lib/navigation";

const SettingsScopeContext = createContext<Scope | null>(null);

export const SettingsScopeProvider: React.FC<{
  scope: Scope;
  children: React.ReactNode;
}> = ({ scope, children }) => (
  <SettingsScopeContext.Provider value={scope}>
    {children}
  </SettingsScopeContext.Provider>
);

/** The scope of the page around, or `null` outside one. */
export const useSettingsScope = (): Scope | null =>
  useContext(SettingsScopeContext);

/** How a setting row shows inside the list of deviating profiles under a
 * default (P7.1): the profile's name instead of the setting's title, and no
 * description. `SettingContainer` reads it. */
export interface SettingRowOverride {
  title: React.ReactNode;
}

const SettingRowContext = createContext<SettingRowOverride | null>(null);

export const SettingRowOverrideProvider: React.FC<{
  value: SettingRowOverride;
  children: React.ReactNode;
}> = ({ value, children }) => (
  <SettingRowContext.Provider value={value}>
    {children}
  </SettingRowContext.Provider>
);

export const useSettingRowOverride = (): SettingRowOverride | null =>
  useContext(SettingRowContext);

/** Opens a page of a profile (P7.2 "Open in …", P5 the page "Profile"'s
 * list); the settings page provides it. With `jump`, the page then scrolls
 * to that setting and lights it up (qa 2026-11-22_1603, B5). */
export type OpenProfilePage = (
  profileId: string,
  page: string,
  jump?: JumpTarget,
) => void;

const OpenProfilePageContext = createContext<OpenProfilePage | null>(null);

export const OpenProfilePageProvider: React.FC<{
  value: OpenProfilePage;
  children: React.ReactNode;
}> = ({ value, children }) => (
  <OpenProfilePageContext.Provider value={value}>
    {children}
  </OpenProfilePageContext.Provider>
);

export const useOpenProfilePage = (): OpenProfilePage | null =>
  useContext(OpenProfilePageContext);

/** Shows another place of the settings window, e.g. a page of the same
 * scope (`scopePage`); the settings page provides it. */
export type OpenLocation = (location: NavLocation) => void;

const OpenLocationContext = createContext<OpenLocation | null>(null);

export const OpenLocationProvider: React.FC<{
  value: OpenLocation;
  children: React.ReactNode;
}> = ({ value, children }) => (
  <OpenLocationContext.Provider value={value}>
    {children}
  </OpenLocationContext.Provider>
);

/** How to show another place, or `null` outside the settings window. */
export const useOpenLocation = (): OpenLocation | null =>
  useContext(OpenLocationContext);
