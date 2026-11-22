// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settingsAreas.test.ts
//
// DanyWhere: the settings in three areas (qa 2026-11-19_1711 P6, P11; plan
// 2026-11-19_1910 "Zuordnung der Einstellungen"). Each setting is drawn on
// exactly one page, the one its area gives it. The pages import every setting
// (and through them i18n's Vite-only glob), so they can't be loaded here; the
// checks read their source instead.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

/** A source file next to this one; empty when it doesn't exist. */
const read = (path: string) => {
  const url = new URL(path, import.meta.url);
  return existsSync(url) ? readFileSync(url, "utf8") : "";
};

// Every check runs, so a run reports how many fail, not just the first.
let total = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  total += 1;
  try {
    body();
  } catch (error) {
    failures.push(`${name}: ${(error as Error).message.split("\n")[0]}`);
  }
};

/** Every page of the settings window, by the file that draws it. */
const PAGES = {
  defaultsGeneral: "./settings/general/GeneralSettings.tsx",
  defaultsModels: "./settings/models/ModelsSettings.tsx",
  defaultsPostProcessing:
    "./settings/post-processing/PostProcessingSettings.tsx",
  defaultsAdvanced: "./settings/advanced/AdvancedSettings.tsx",
  appGeneral: "./settings/application/ApplicationGeneralSettings.tsx",
  appUpdates: "./settings/application/UpdatesSettings.tsx",
  appStorage: "./settings/application/StorageSettings.tsx",
  appAccessibility: "./settings/application/AccessibilitySettings.tsx",
  appDebug: "./settings/debug/DebugSettings.tsx",
  about: "./settings/about/AboutSettings.tsx",
} as const;
type Page = keyof typeof PAGES;
const source = Object.fromEntries(
  Object.entries(PAGES).map(([page, path]) => [page, read(path)]),
) as Record<Page, string>;

/** Where each setting is drawn: its component and its page. */
const PLACES: [component: string, page: Page][] = [
  // Application settings › General (P11)
  ["AutostartToggle", "appGeneral"],
  ["StartHidden", "appGeneral"],
  ["ShowTrayIcon", "appGeneral"],
  ["AppLanguageSelector", "appGeneral"],
  ["ThemeSelector", "appGeneral"],
  // Application settings › Updates
  ["UpdateChecksToggle", "appUpdates"],
  ["ShowWhatsNewOnUpdate", "appUpdates"],
  // Application settings › Storage
  ["RecordingStorageLimitSetting", "appStorage"],
  ["ModelUnloadTimeoutSetting", "appStorage"],
  ["AppDataDirectory", "appStorage"],
  ["LogDirectory", "appStorage"],
  // Application settings › Accessibility. The mode without Accessibility has
  // no control of its own (chosen in onboarding); its two settings move here.
  ["KeyboardImplementationSelector", "appAccessibility"],
  ["DictationKeyInserts", "appAccessibility"],
  ["DictationServiceWhenIdleSetting", "appAccessibility"],
  // Application settings › Debug keeps its own
  ["LogLevelSelector", "appDebug"],
  ["WordCorrectionThreshold", "appDebug"],
  ["PasteDelay", "appDebug"],
  ["ReliablePasteToggle", "appDebug"],
  ["HoldThreshold", "appDebug"],
  ["RecordingBuffer", "appDebug"],
  ["AccelerationSelector", "appDebug"],
  ["LazyStreamClose", "appDebug"],
  ["VadBackendSelector", "appDebug"],
  ["LiveLogViewer", "appDebug"],
  // Defaults: what concerns a dictation, also when it was on Debug
  ["ShowOverlay", "defaultsAdvanced"],
  ["LanguageDetection", "defaultsAdvanced"],
  ["LanguageDetectionWindow", "defaultsAdvanced"],
  ["SoundPicker", "defaultsAdvanced"],
  ["AlwaysOnMicrophone", "defaultsAdvanced"],
  ["ClamshellMicrophoneSelector", "defaultsAdvanced"],
  ["AirPodsMode", "defaultsAdvanced"],
  ["PasteMethodSetting", "defaultsAdvanced"],
  ["MicrophoneSelector", "defaultsGeneral"],
  ["PreferredLanguages", "defaultsGeneral"],
];

/** How often a page draws `component` (`<Name` followed by a blank, `/` or a line end). */
const draws = (text: string, component: string) =>
  [...text.matchAll(new RegExp(`<${component}(?=[\\s/>])`, "g"))].length;

check("every page file is found", () => {
  const missing = Object.entries(source)
    .filter(([, text]) => !text)
    .map(([page]) => page);
  assert.deepEqual(missing, []);
});

for (const [component, page] of PLACES) {
  check(`${component} is drawn on ${page} only`, () => {
    const where = (Object.keys(PAGES) as Page[]).filter(
      (p) => draws(source[p], component) > 0,
    );
    assert.deepEqual(where, [page]);
  });
}

// The language detection window sits right under the language detection.
check("the detection window follows the language detection", () => {
  const advanced = source.defaultsAdvanced;
  const detection = advanced.search(/<LanguageDetection(?=[\s/>])/);
  const window = advanced.search(/<LanguageDetectionWindow(?=[\s/>])/);
  assert.ok(detection >= 0 && window > detection, `${detection} ${window}`);
});

// --- The sidebar --------------------------------------------------------
const sidebar = read("./Sidebar.tsx");
const selector = read("./settings/profiles/ProfileSelector.tsx");
const content = read("./SettingsPage.tsx");
const general = source.defaultsGeneral;
const profilesPage = read("./settings/profiles/ProfilesPage.tsx");

check("the sidebar draws the tree of the navigation", () =>
  assert.match(sidebar, /navTree\(/),
);
check("the sidebar no longer lists flat sections", () =>
  assert.doesNotMatch(sidebar, /SECTIONS_CONFIG/),
);
check("the box at the top is called In Use", () =>
  assert.match(selector, /t\("sidebar\.inUse"\)/),
);
check("choosing by application stays under it", () =>
  assert.match(selector, /<ProfileAutoByAppCompact \/>/),
);
check("the overview of the profiles is today's page", () =>
  assert.match(content, /<ProfilesPage \/>/),
);
// Strand E2: a profile's pages are the profile view.
check("a profile's page Profile is its overview", () =>
  assert.match(content, /<ProfileOverview\b/),
);
check("a profile's other pages are the defaults' pages in its scope", () =>
  assert.match(content, /<ProfileScopedPage\b/),
);
check("the defaults' pages read and write the defaults", () =>
  assert.match(content, /<SettingsScopeProvider scope=\{DEFAULTS_SCOPE\}>/),
);
check("General no longer manages the profiles", () =>
  assert.doesNotMatch(general, /<ProfilesSettings\b|ProfilesSettings"/),
);
check("the page of the profiles manages them", () =>
  assert.match(profilesPage, /<ProfilesSettings \/>/),
);

// --- The user's words ---------------------------------------------------
const en = JSON.parse(read("../i18n/locales/en/translation.json"));
const de = JSON.parse(read("../i18n/locales/de/translation.json"));
check("the areas carry the user's names", () =>
  assert.deepEqual(de.sidebar?.areas, {
    defaults: "Grundeinstellungen",
    profiles: "Profileinstellungen",
    application: "Applikationseinstellungen",
  }),
);
check("the English areas", () =>
  assert.deepEqual(en.sidebar?.areas, {
    defaults: "Defaults",
    profiles: "Profile Settings",
    application: "Application Settings",
  }),
);
check("the box and the new pages carry the user's names", () => {
  assert.equal(de.sidebar?.inUse, "In Verwendung");
  assert.equal(de.sidebar?.updates, "Updates");
  assert.equal(de.sidebar?.storage, "Speicher");
  assert.equal(de.sidebar?.profile, "Profil");
  assert.equal(
    de.onboarding?.permissions?.accessibility?.name,
    "Bedienungshilfen",
  );
});

if (failures.length > 0) {
  console.error(
    `settingsAreas.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`settingsAreas.test.ts: all ${total} checks passed`);
