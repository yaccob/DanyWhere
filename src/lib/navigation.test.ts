// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/navigation.test.ts
//
// DanyWhere: the sidebar as a tree of three areas (qa 2026-11-19_1711 P3, P6,
// P11): which pages each area has, what a click shows, what the window shows
// once a page or a profile is gone, and the rows of the tree.
import assert from "node:assert/strict";
import {
  APPLICATION_PAGES,
  DEFAULTS_PAGES,
  PROFILE_PAGES,
  START_LOCATION,
  applicationPages,
  areaLabelKey,
  defaultsPages,
  edgeToEdge,
  fillsWindow,
  locationKey,
  navContext,
  navPath,
  navTree,
  showsPath,
  pageLabelKey,
  profilePages,
  resolveLocation,
  sameLocation,
  scopePage,
  selectArea,
  selectProfile,
  type NavContext,
  type NavItem,
  type NavLocation,
} from "./navigation";

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

const settings = {
  profiles: [
    { id: "default", values: { post_process_enabled: true } },
    { id: "work", values: { post_process_enabled: true } },
    { id: "private", values: { post_process_enabled: false } },
  ],
  post_process_enabled: true,
  debug_mode: false,
};
const ctx: NavContext = navContext(settings);
const noPostProcess = navContext({ ...settings, post_process_enabled: false });
const debug = navContext({ ...settings, debug_mode: true });
const noProfiles = navContext({ ...settings, profiles: [] });

// --- The context, read from the settings ---------------------------------
check("the profile settings show once there are profiles", () =>
  assert.equal(ctx.profilesEnabled, true),
);
// Without any profile the area still shows: a new one is made in its
// overview, and the defaults are no profile (P1).
check("no profile yet, the profile settings still show", () =>
  assert.equal(noProfiles.profilesEnabled, true),
);
check("missing settings: no profile settings", () =>
  assert.equal(navContext(null).profilesEnabled, false),
);
// "Default" becomes the defaults (P1): it is the area "Defaults" itself, not
// a profile under the profile settings.
check("the tree lists the profiles without the defaults' one", () =>
  assert.deepEqual(
    ctx.profiles.map((profile) => profile.id),
    ["work", "private"],
  ),
);
check("a profile's post-processing comes from its own values", () =>
  assert.deepEqual(
    ctx.profiles.map((profile) => profile.postProcessEnabled),
    [true, false],
  ),
);
check("a profile without values counts as without post-processing", () =>
  assert.equal(
    navContext({ profiles: [{ id: "x" }] }).profiles[0]?.postProcessEnabled,
    false,
  ),
);
check("post-processing and debug mode come from the settings", () => {
  assert.equal(ctx.postProcessEnabled, true);
  assert.equal(ctx.debugMode, false);
  assert.equal(debug.debugMode, true);
});
// The flat fields hold the values of whatever is in use; the defaults' pages
// edit the defaults, so their Post Process page follows the defaults' value.
const workInUse = navContext({
  ...settings,
  post_process_enabled: false,
  default_values: { post_process_enabled: true },
});
const defaultsOff = navContext({
  ...settings,
  post_process_enabled: true,
  default_values: { post_process_enabled: false },
});
check("a profile without post-processing in use keeps the defaults' page", () =>
  assert.equal(workInUse.postProcessEnabled, true),
);
check("defaults without post-processing hide it while a profile has it", () =>
  assert.equal(defaultsOff.postProcessEnabled, false),
);
check("the defaults' pages follow the defaults, not what is in use", () => {
  assert.ok(defaultsPages(workInUse).includes("postprocessing"));
  assert.ok(!defaultsPages(defaultsOff).includes("postprocessing"));
});

// --- Which pages each area has --------------------------------------------
check("the defaults have today's four pages, in order", () =>
  assert.deepEqual(defaultsPages(ctx), [
    "general",
    "models",
    "postprocessing",
    "advanced",
  ]),
);
check("the defaults hide Post Process while post-processing is off", () =>
  assert.deepEqual(defaultsPages(noPostProcess), [
    "general",
    "models",
    "advanced",
  ]),
);
check("a profile has the page Profile, then the defaults' pages", () =>
  assert.deepEqual(profilePages(ctx.profiles[0]), [
    "profile",
    "general",
    "models",
    "postprocessing",
    "advanced",
  ]),
);
check("a profile hides Post Process while its own is off", () =>
  assert.deepEqual(profilePages(ctx.profiles[1]), [
    "profile",
    "general",
    "models",
    "advanced",
  ]),
);
check("the application settings have their pages without debug mode", () =>
  assert.deepEqual(applicationPages(ctx), [
    "general",
    "updates",
    "storage",
    "accessibility",
  ]),
);
check("debug mode adds Debug to the application settings", () =>
  assert.deepEqual(applicationPages(debug), [
    "general",
    "updates",
    "storage",
    "accessibility",
    "debug",
  ]),
);
check("the page lists name every page there is", () => {
  assert.deepEqual([...DEFAULTS_PAGES], defaultsPages(ctx));
  assert.deepEqual(
    [...PROFILE_PAGES],
    ["profile", "general", "models", "postprocessing", "advanced"],
  );
  assert.deepEqual([...APPLICATION_PAGES], applicationPages(debug));
});

// --- Labels ---------------------------------------------------------------
check("pages of the defaults reuse today's sidebar labels", () => {
  assert.equal(pageLabelKey("general"), "sidebar.general");
  assert.equal(pageLabelKey("models"), "sidebar.models");
  assert.equal(pageLabelKey("postprocessing"), "sidebar.postProcessing");
  assert.equal(pageLabelKey("advanced"), "sidebar.advanced");
});
check("a profile's pages read like the defaults', plus Profile", () => {
  assert.equal(pageLabelKey("profile"), "sidebar.profile");
  assert.equal(pageLabelKey("models"), "sidebar.models");
});
check("the application pages have their labels", () => {
  assert.equal(pageLabelKey("general"), "sidebar.general");
  assert.equal(pageLabelKey("updates"), "sidebar.updates");
  assert.equal(pageLabelKey("storage"), "sidebar.storage");
  assert.equal(
    pageLabelKey("accessibility"),
    "onboarding.permissions.accessibility.name",
  );
  assert.equal(pageLabelKey("debug"), "sidebar.debug");
});
check("the areas have their labels", () => {
  assert.equal(areaLabelKey("defaults"), "sidebar.areas.defaults");
  assert.equal(areaLabelKey("profiles"), "sidebar.areas.profiles");
  assert.equal(areaLabelKey("application"), "sidebar.areas.application");
  assert.equal(areaLabelKey("history"), "sidebar.history");
  assert.equal(areaLabelKey("manual"), "sidebar.manual");
  assert.equal(areaLabelKey("about"), "sidebar.about");
});

// --- Keys -----------------------------------------------------------------
const everyLocation: NavLocation[] = [
  ...DEFAULTS_PAGES.map((page) => ({ kind: "defaults" as const, page })),
  { kind: "profiles" },
  ...PROFILE_PAGES.map((page) => ({
    kind: "profile" as const,
    profileId: "work",
    page,
  })),
  ...PROFILE_PAGES.map((page) => ({
    kind: "profile" as const,
    profileId: "private",
    page,
  })),
  ...APPLICATION_PAGES.map((page) => ({ kind: "application" as const, page })),
  { kind: "history" },
  { kind: "manual" },
  { kind: "about" },
];
check("every place has a key of its own", () => {
  const keys = everyLocation.map(locationKey);
  assert.equal(new Set(keys).size, keys.length, keys.join(" "));
  assert.ok(
    keys.every((key) => key.length > 0),
    "a key is empty",
  );
});
check("General of the defaults and of the application differ", () =>
  assert.equal(
    sameLocation(
      { kind: "defaults", page: "general" },
      { kind: "application", page: "general" },
    ),
    false,
  ),
);
check("the same place is the same", () =>
  assert.equal(
    sameLocation(
      { kind: "profile", profileId: "work", page: "models" },
      { kind: "profile", profileId: "work", page: "models" },
    ),
    true,
  ),
);

// --- What a click shows ---------------------------------------------------
check("the window opens on the defaults' General", () =>
  assert.deepEqual(START_LOCATION, { kind: "defaults", page: "general" }),
);
check("a click on Defaults shows its first page", () =>
  assert.deepEqual(selectArea("defaults"), {
    kind: "defaults",
    page: "general",
  }),
);
check("a click on Profile settings shows the overview of all profiles", () =>
  assert.deepEqual(selectArea("profiles"), { kind: "profiles" }),
);
check("a click on Application settings shows its first page", () =>
  assert.deepEqual(selectArea("application"), {
    kind: "application",
    page: "general",
  }),
);
check("a click on History or About shows it", () => {
  assert.deepEqual(selectArea("history"), { kind: "history" });
  assert.deepEqual(selectArea("about"), { kind: "about" });
});
check("a click on Manual shows the manual's home page", () =>
  assert.deepEqual(selectArea("manual"), { kind: "manual" }),
);
check("a chapter of the manual is the same place as its home page", () =>
  assert.equal(
    sameLocation(
      { kind: "manual" },
      { kind: "manual", page: "how-it-works/start-and-end-shortcuts/" },
    ),
    true,
  ),
);
check("the manual stays when the settings change", () => {
  const chapter: NavLocation = {
    kind: "manual",
    page: "how-it-works/start-and-end-shortcuts/",
  };
  assert.deepEqual(resolveLocation(chapter, ctx), chapter);
});
check("a click on a profile from elsewhere opens its page Profile", () =>
  assert.deepEqual(
    selectProfile("work", { kind: "defaults", page: "models" }, ctx),
    { kind: "profile", profileId: "work", page: "profile" },
  ),
);
check("from one profile to another, the page stays", () =>
  assert.deepEqual(
    selectProfile(
      "work",
      { kind: "profile", profileId: "private", page: "models" },
      ctx,
    ),
    { kind: "profile", profileId: "work", page: "models" },
  ),
);
check("a page the other profile lacks falls back to Profile", () =>
  assert.deepEqual(
    selectProfile(
      "private",
      { kind: "profile", profileId: "work", page: "postprocessing" },
      ctx,
    ),
    { kind: "profile", profileId: "private", page: "profile" },
  ),
);

// --- What the window shows once something is gone --------------------------
check("an existing place stays", () => {
  for (const location of everyLocation.filter(
    (l) =>
      !(l.kind === "profile" && l.profileId === "private") &&
      !(l.kind === "application" && l.page === "debug"),
  )) {
    assert.deepEqual(resolveLocation(location, ctx), location);
  }
});
check("a removed profile shows the overview of all profiles", () =>
  assert.deepEqual(
    resolveLocation(
      { kind: "profile", profileId: "gone", page: "models" },
      ctx,
    ),
    { kind: "profiles" },
  ),
);
check("the defaults' profile has no pages of its own: overview", () =>
  assert.deepEqual(
    resolveLocation(
      { kind: "profile", profileId: "default", page: "general" },
      ctx,
    ),
    { kind: "profiles" },
  ),
);
check("a profile's hidden Post Process shows its page Profile", () =>
  assert.deepEqual(
    resolveLocation(
      { kind: "profile", profileId: "private", page: "postprocessing" },
      ctx,
    ),
    { kind: "profile", profileId: "private", page: "profile" },
  ),
);
check("the defaults' hidden Post Process shows their General", () =>
  assert.deepEqual(
    resolveLocation(
      { kind: "defaults", page: "postprocessing" },
      noPostProcess,
    ),
    { kind: "defaults", page: "general" },
  ),
);
check("Debug without debug mode shows the application's General", () =>
  assert.deepEqual(
    resolveLocation({ kind: "application", page: "debug" }, ctx),
    { kind: "application", page: "general" },
  ),
);
check("Debug in debug mode stays", () =>
  assert.deepEqual(
    resolveLocation({ kind: "application", page: "debug" }, debug),
    { kind: "application", page: "debug" },
  ),
);
check(
  "before the settings are loaded, the profile settings show the start",
  () => {
    const notLoaded = navContext(null);
    assert.deepEqual(
      resolveLocation({ kind: "profiles" }, notLoaded),
      START_LOCATION,
    );
    assert.deepEqual(
      resolveLocation(
        { kind: "profile", profileId: "work", page: "models" },
        notLoaded,
      ),
      START_LOCATION,
    );
  },
);
check(
  "without a profile, the overview stays and a gone one leads there",
  () => {
    assert.deepEqual(resolveLocation({ kind: "profiles" }, noProfiles), {
      kind: "profiles",
    });
    assert.deepEqual(
      resolveLocation(
        { kind: "profile", profileId: "work", page: "models" },
        noProfiles,
      ),
      { kind: "profiles" },
    );
  },
);

// --- The rows of the tree -------------------------------------------------
const ids = (items: NavItem[]) => items.map((item) => item.id);
const current = (items: NavItem[]) =>
  items.filter((item) => item.current).map((item) => item.id);
const atStart = navTree(ctx, START_LOCATION);
const inWork = navTree(ctx, {
  kind: "profile",
  profileId: "work",
  page: "models",
});

check(
  "the tree: three areas, their pages, then History, Manual and About",
  () =>
    assert.deepEqual(ids(atStart), [
      "defaults",
      "defaults/general",
      "defaults/models",
      "defaults/postprocessing",
      "defaults/advanced",
      "profiles",
      "profile/work",
      "profile/private",
      "application",
      "application/general",
      "application/updates",
      "application/storage",
      "application/accessibility",
      "history",
      "manual",
      "about",
    ]),
);
check("the chosen profile is open, with its pages under it", () =>
  assert.deepEqual(ids(inWork).slice(5, 13), [
    "profiles",
    "profile/work",
    "profile/work/profile",
    "profile/work/general",
    "profile/work/models",
    "profile/work/postprocessing",
    "profile/work/advanced",
    "profile/private",
  ]),
);
check("only the chosen profile is open", () =>
  assert.deepEqual(
    inWork.filter((item) => item.open).map((item) => item.id),
    ["profile/work"],
  ),
);
check("one row is current: the page shown", () => {
  assert.deepEqual(current(atStart), ["defaults/general"]);
  assert.deepEqual(current(inWork), ["profile/work/models"]);
});
check("the overview makes the area row current", () =>
  assert.deepEqual(current(navTree(ctx, { kind: "profiles" })), ["profiles"]),
);
check("History, Manual and About are current when shown", () => {
  assert.deepEqual(current(navTree(ctx, { kind: "history" })), ["history"]);
  assert.deepEqual(current(navTree(ctx, { kind: "manual" })), ["manual"]);
  assert.deepEqual(current(navTree(ctx, { kind: "about" })), ["about"]);
});
check("Manual is current at any of its chapters", () =>
  assert.deepEqual(
    current(
      navTree(ctx, {
        kind: "manual",
        page: "how-it-works/start-and-end-shortcuts/",
      }),
    ),
    ["manual"],
  ),
);
check("levels: areas 0, pages and profiles 1, a profile's pages 2", () => {
  const level = (id: string) => inWork.find((item) => item.id === id)?.level;
  assert.equal(level("defaults"), 0);
  assert.equal(level("defaults/models"), 1);
  assert.equal(level("profile/work"), 1);
  assert.equal(level("profile/work/models"), 2);
  assert.equal(level("application/storage"), 1);
  assert.equal(level("manual"), 0);
  assert.equal(level("about"), 0);
});
check("areas, History and About carry their icon, other rows none", () => {
  const icons = inWork
    .filter((item) => item.icon)
    .map((item) => `${item.id}:${item.icon}`);
  assert.deepEqual(icons, [
    "defaults:defaults",
    "profiles:profiles",
    "application:application",
    "history:history",
    "manual:manual",
    "about:about",
  ]);
});
check("each row's click target is what selecting it shows", () => {
  const target = (id: string) => inWork.find((item) => item.id === id)?.target;
  assert.deepEqual(target("defaults"), selectArea("defaults"));
  assert.deepEqual(target("profiles"), { kind: "profiles" });
  assert.deepEqual(target("defaults/advanced"), {
    kind: "defaults",
    page: "advanced",
  });
  assert.deepEqual(target("profile/private"), {
    kind: "profile",
    profileId: "private",
    page: "models",
  });
  assert.deepEqual(target("application/updates"), {
    kind: "application",
    page: "updates",
  });
});
check("rows are labelled by key, profiles by their id", () => {
  const label = (id: string) => inWork.find((item) => item.id === id)?.label;
  assert.deepEqual(label("defaults"), { key: "sidebar.areas.defaults" });
  assert.deepEqual(label("profile/work"), { profileId: "work" });
  assert.deepEqual(label("profile/work/profile"), { key: "sidebar.profile" });
  assert.deepEqual(label("application/storage"), { key: "sidebar.storage" });
});
check("row ids are unique", () => {
  const all = ids(
    navTree(debug, { kind: "profile", profileId: "work", page: "profile" }),
  );
  assert.equal(new Set(all).size, all.length);
});
check("before the settings are loaded the tree has no profile settings", () =>
  assert.equal(
    ids(navTree(navContext(null), START_LOCATION)).some((id) =>
      id.startsWith("profile"),
    ),
    false,
  ),
);
check("without a profile the tree has the area but no profile in it", () => {
  const without = ids(navTree(noProfiles, START_LOCATION));
  assert.ok(without.some((id) => id.startsWith("profile")));
  assert.equal(
    without.some((id) => id.startsWith("profile/")),
    false,
  );
});
check("debug mode adds the row Debug", () =>
  assert.ok(ids(navTree(debug, START_LOCATION)).includes("application/debug")),
);

// --- The path above a page --------------------------------------------------
check("the path names the area and the page", () => {
  assert.deepEqual(navPath({ kind: "defaults", page: "advanced" }), [
    { key: "sidebar.areas.defaults" },
    { key: "sidebar.advanced" },
  ]);
  assert.deepEqual(navPath({ kind: "application", page: "general" }), [
    { key: "sidebar.areas.application" },
    { key: "sidebar.general" },
  ]);
});
check("a profile's path names the profile between area and page", () =>
  assert.deepEqual(
    navPath({ kind: "profile", profileId: "work", page: "advanced" }),
    [
      { key: "sidebar.areas.profiles" },
      { profileId: "work" },
      { key: "sidebar.advanced" },
    ],
  ),
);
check("the overview, History and About have a path of one", () => {
  assert.deepEqual(navPath({ kind: "profiles" }), [
    { key: "sidebar.areas.profiles" },
  ]);
  assert.deepEqual(navPath({ kind: "history" }), [{ key: "sidebar.history" }]);
  assert.deepEqual(navPath({ kind: "about" }), [{ key: "sidebar.about" }]);
  assert.deepEqual(navPath({ kind: "manual" }), [{ key: "sidebar.manual" }]);
});

// --- Layout -----------------------------------------------------------------
check("Models fills the window, other pages don't", () => {
  assert.equal(fillsWindow({ kind: "defaults", page: "models" }), true);
  assert.equal(fillsWindow({ kind: "defaults", page: "general" }), false);
});
check("the manual fills the window, edge to edge", () => {
  assert.equal(fillsWindow({ kind: "manual" }), true);
  assert.equal(edgeToEdge({ kind: "manual" }), true);
  assert.equal(
    edgeToEdge({
      kind: "manual",
      page: "how-it-works/start-and-end-shortcuts/",
    }),
    true,
  );
});
check("every other page keeps its margin", () => {
  for (const location of everyLocation) {
    if (location.kind === "manual") continue;
    assert.equal(edgeToEdge(location), false, locationKey(location));
  }
});

// --- Counts of own values (P5) --------------------------------------------
const counted = navContext({
  ...settings,
  profiles: [
    {
      id: "work",
      values: { post_process_enabled: true },
      own: ["paste_method", "selected_model"],
      own_entries: {
        bindings: { own: ["copy_last_transcript"], off: [] },
        word_lists: { own: [], off: ["names"] },
      },
    },
    { id: "private", values: { post_process_enabled: false }, own: [] },
  ],
});
check("a profile counts its own values, entries included", () => {
  assert.equal(counted.profiles[0].ownCount, 4);
  assert.equal(counted.profiles[1].ownCount, 0);
});
check("a profile counts its own values per page", () =>
  assert.deepEqual(counted.profiles[0].pageCounts, {
    general: 1,
    models: 1,
    advanced: 2,
  }),
);
check("settings from before the defaults count nothing", () =>
  assert.equal(ctx.profiles[0].ownCount, 0),
);
check("the tree shows a profile's count and its pages' counts", () => {
  const tree = navTree(counted, {
    kind: "profile",
    profileId: "work",
    page: "general",
  });
  const count = (id: string) => tree.find((item) => item.id === id)?.count;
  assert.equal(count("profile/work"), 4);
  assert.equal(count("profile/work/models"), 1);
  assert.equal(count("profile/work/general"), 1);
  assert.equal(count("profile/work/advanced"), 2);
  assert.equal(count("profile/work/postprocessing"), undefined);
  assert.equal(count("profile/work/profile"), undefined);
  assert.equal(count("profile/private"), undefined);
  assert.equal(count("defaults/general"), undefined);
});
check("a profile's Models fills the window like the defaults'", () =>
  assert.equal(
    fillsWindow({ kind: "profile", profileId: "work", page: "models" }),
    true,
  ),
);

// --- Which pages show the path (qa 2026-11-22_0107, A3) ---------------------
check("every page of the three areas shows its path, the overview too", () => {
  assert.equal(showsPath({ kind: "defaults", page: "general" }), true);
  assert.equal(showsPath({ kind: "profiles" }), true);
  assert.equal(
    showsPath({ kind: "profile", profileId: "work", page: "profile" }),
    true,
  );
  assert.equal(showsPath({ kind: "application", page: "storage" }), true);
});
check("History, Manual and About, outside the areas, show none", () => {
  assert.equal(showsPath({ kind: "history" }), false);
  assert.equal(showsPath({ kind: "manual" }), false);
  assert.equal(showsPath({ kind: "about" }), false);
});

// --- A page of the same scope (word lists: "Set Up Post-Processing") ------
check("a page of the defaults' scope is the defaults' page", () =>
  assert.deepEqual(scopePage({ kind: "defaults" }, "postprocessing"), {
    kind: "defaults",
    page: "postprocessing",
  }),
);
check("a page of a profile's scope is that profile's page", () =>
  assert.deepEqual(
    scopePage({ kind: "profile", id: "profile_1795300000001" }, "models"),
    { kind: "profile", profileId: "profile_1795300000001", page: "models" },
  ),
);

if (failures.length > 0) {
  console.error(
    `navigation.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`navigation.test.ts: all ${total} checks passed`);
