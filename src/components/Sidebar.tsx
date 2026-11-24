import React from "react";
import { useTranslation } from "react-i18next";
import {
  BookOpen,
  History,
  Info,
  Layers,
  MonitorCog,
  SlidersHorizontal,
  type LucideIcon,
} from "lucide-react";
import HandyTextLogo from "./icons/HandyTextLogo";
import { useSettings } from "../hooks/useSettings";
// DanyWhere: the profile in use, on every page ("In Use", qa 2026-11-19_1711 P2).
import { ProfileSelector } from "./settings/profiles/ProfileSelector";
import { useProfiles } from "@/hooks/useProfiles";
import { useOpenManual } from "@/hooks/useOpenManual";
// DanyWhere: names a test build (Dev, NoAX) under the wordmark.
import VariantBadge from "./VariantBadge";
// DanyWhere: the tree of three areas (P3, P6, P11).
import {
  navContext,
  navTree,
  type NavIcon,
  type NavItem,
  type NavLocation,
} from "@/lib/navigation";

const ICONS: Record<NavIcon, LucideIcon> = {
  defaults: SlidersHorizontal,
  profiles: Layers,
  application: MonitorCog,
  history: History,
  about: Info,
};

/** Rows of an area's pages line up with the area's name, a profile's pages
 * one step further in. */
const INDENT = ["ps-2", "ps-[34px]", "ps-[46px]"] as const;

/** What a row's look depends on. */
type RowLook = Pick<NavItem, "level" | "current" | "open" | "count" | "icon">;

/** DanyWhere: the user manual's row looks like History's and About's, a
 * single page's, but is never current: it opens a window of its own. */
const MANUAL_ROW: RowLook = { level: 0, current: false };

interface NavRowProps {
  item: RowLook;
  label: string;
  onClick: () => void;
  /** DanyWhere: an icon of its own, for a row that is no place of the
   * tree (the manual). */
  icon?: LucideIcon;
}

const NavRow: React.FC<NavRowProps> = ({ item, label, onClick, icon }) => {
  const Icon = icon ?? (item.icon ? ICONS[item.icon] : null);
  // The three areas head their pages; History and About are single pages.
  const heading =
    item.icon === "defaults" ||
    item.icon === "profiles" ||
    item.icon === "application";
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={item.current ? "page" : undefined}
      aria-expanded={item.open === undefined ? undefined : item.open}
      title={label}
      className={`flex w-full items-center gap-2 rounded-lg pe-2 text-start cursor-pointer transition-colors ${INDENT[item.level]} ${
        item.level === 0 ? "py-1.5" : "py-1"
      } ${heading ? "mt-2 first:mt-0" : ""} ${
        item.current
          ? "bg-logo-primary/80"
          : "hover:bg-line hover:opacity-100 opacity-85"
      }`}
    >
      {Icon && <Icon width={18} height={18} className="shrink-0" />}
      <span
        className={`min-w-0 text-sm ${
          heading
            ? "font-semibold leading-tight break-words hyphens-auto"
            : `truncate ${item.open ? "font-semibold" : "font-medium"}`
        }`}
      >
        {label}
      </span>
      {/* DanyWhere (strand E2): how many own values a profile or its page
          has (P5). */}
      {item.count !== undefined && (
        <span
          className={`ms-auto shrink-0 text-xs font-semibold tabular-nums ${
            item.current ? "" : "text-logo-primary"
          }`}
        >
          {item.count}
        </span>
      )}
    </button>
  );
};

interface SidebarProps {
  /** What the content area shows, already resolved (`resolveLocation`). */
  location: NavLocation;
  onNavigate: (location: NavLocation) => void;
}

/** DanyWhere: the sidebar. On top the profile in use ("In Use"), below it
 * the tree: Defaults, Profile settings (the overview, then each profile, the
 * chosen one open with its pages), Application settings, History, Manual,
 * About. */
export const Sidebar: React.FC<SidebarProps> = ({ location, onNavigate }) => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  const { profiles, displayName } = useProfiles();
  const openManual = useOpenManual();
  const items = navTree(navContext(settings), location);

  const label = (item: NavItem): string => {
    if ("key" in item.label) return t(item.label.key);
    const id = item.label.profileId;
    const profile = profiles.find((p) => p.id === id);
    return profile ? displayName(profile) : id;
  };

  return (
    <div className="flex flex-col w-52 h-full border-e border-line px-2">
      <div className="flex flex-col items-center shrink-0">
        <HandyTextLogo width={120} className="m-4" />
        <VariantBadge className="-mt-2 mb-3 text-xs" />
      </div>
      <ProfileSelector />
      {/* The language lets long German names break at a syllable. */}
      <nav
        lang={i18n.language}
        className="flex-1 min-h-0 overflow-y-auto flex flex-col w-full gap-0.5 pb-3"
      >
        {items.map((item) => (
          <React.Fragment key={item.id}>
            {/* DanyWhere: the user manual, between History and About. */}
            {item.id === "about" && (
              <NavRow
                item={MANUAL_ROW}
                label={t("sidebar.manual")}
                icon={BookOpen}
                onClick={() => void openManual()}
              />
            )}
            <NavRow
              item={item}
              label={label(item)}
              onClick={() => onNavigate(item.target)}
            />
          </React.Fragment>
        ))}
      </nav>
    </div>
  );
};
