// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/sidebarSections.test.ts
//
// DanyWhere: where the profiles are managed, on a page "Profiles" of their
// own right after General. Sidebar.tsx imports every settings page (and
// through them i18n's Vite-only glob), so it can't be loaded here; the
// checks read its source instead.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

/** A source file next to this one; empty when it doesn't exist. */
const read = (path: string) => {
  const url = new URL(path, import.meta.url);
  return existsSync(url) ? readFileSync(url, "utf8") : "";
};

const sidebar = read("./Sidebar.tsx");
const general = read("./settings/general/GeneralSettings.tsx");
const profilesPage = read("./settings/profiles/ProfilesPage.tsx");
const en = JSON.parse(read("../i18n/locales/en/translation.json"));

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

/** The body of SECTIONS_CONFIG, between its braces. */
const config = (() => {
  const start = sidebar.indexOf("export const SECTIONS_CONFIG = {");
  const end = sidebar.indexOf("} as const satisfies", start);
  return start < 0 || end < 0 ? "" : sidebar.slice(start, end);
})();

/** The sections in sidebar order: the keys at the config's first level. */
const sections = [...config.matchAll(/^ {2}(\w+): \{$/gm)].map(
  (match) => match[1],
);

/** One section's entry, from its key to its closing brace. */
const entry = (key: string) => {
  const match = config.match(
    new RegExp(`^ {2}${key}: \\{$[\\s\\S]*?^ {2}\\},`, "m"),
  );
  return match ? match[0] : "";
};

check("the sidebar config is found", () =>
  assert.ok(sections.includes("general"), `sections: ${sections.join(",")}`),
);
check("Profiles comes right after General", () =>
  assert.equal(sections[sections.indexOf("general") + 1], "profiles"),
);
check("Profiles has its own label", () =>
  assert.match(entry("profiles"), /labelKey: "sidebar\.profiles",/),
);
check("Profiles shows the page of the profiles", () =>
  assert.match(entry("profiles"), /component: ProfilesPage,/),
);
check("Profiles shows once the settings carry profiles", () =>
  assert.match(entry("profiles"), /enabled: profilesPageEnabled,/),
);
check("the label reads like the page's heading", () =>
  assert.equal(en.sidebar?.profiles, en.settings.profiles.title),
);
check("General no longer manages the profiles", () =>
  assert.doesNotMatch(general, /<ProfilesSettings\b|ProfilesSettings"/),
);
check("the page Profiles manages them", () =>
  assert.match(profilesPage, /<ProfilesSettings \/>/),
);

if (failures.length > 0) {
  console.error(
    `sidebarSections.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`sidebarSections.test.ts: all ${total} checks passed`);
