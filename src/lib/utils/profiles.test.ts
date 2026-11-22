import assert from "node:assert/strict";
import {
  DEFAULT_PROFILE_ID,
  activeProfile,
  canDeleteProfile,
  checkProfileName,
  profileAfterDelete,
  profileDisplayName,
  profilesPageEnabled,
} from "./profiles";

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

// The defaults ("Grundeinstellungen") are no profile; their id stands for
// them where a profile id is expected (in use, switching).
const work = { id: "p1", name: "Work" };
const code = { id: "p2", name: "  Code  " };
const profiles = [work, code];
const defaults = "Grundeinstellungen";

// A profile shows its name without surrounding blanks; a nameless one (only
// "Standard" in a store from before the defaults) shows the given name.
check("defaults id", () => assert.equal(DEFAULT_PROFILE_ID, "default"));
check("empty name shows the given name", () =>
  assert.equal(profileDisplayName({ id: "x", name: "" }, defaults), defaults),
);
check("other name shows as stored", () =>
  assert.equal(profileDisplayName(work, defaults), "Work"),
);
check("name shows trimmed", () =>
  assert.equal(profileDisplayName(code, defaults), "Code"),
);

// The profile in use: the one the id names; none while the defaults are in
// use, or for an unknown id, where the backend puts the defaults in use.
check("in use by id", () => assert.equal(activeProfile(profiles, "p1"), work));
check("the defaults in use: no profile", () =>
  assert.equal(activeProfile(profiles, DEFAULT_PROFILE_ID), null),
);
check("unknown id: no profile", () =>
  assert.equal(activeProfile(profiles, "gone"), null),
);
check("null id: no profile", () =>
  assert.equal(activeProfile(profiles, null), null),
);
check("no profiles, none in use", () =>
  assert.equal(activeProfile([], "p1"), null),
);

// The page "Profiles" shows once the settings are loaded, also without any
// profile: a new one is made there.
check("profiles page: settings not loaded", () =>
  assert.equal(profilesPageEnabled(null), false),
);
check("profiles page: settings without profiles", () =>
  assert.equal(profilesPageEnabled({}), false),
);
check("profiles page: no profile yet", () =>
  assert.equal(profilesPageEnabled({ profiles: [] }), true),
);
check("profiles page: several profiles", () =>
  assert.equal(profilesPageEnabled({ profiles }), true),
);

// Every profile can be deleted, the last one too; the defaults can't.
check("delete one of several", () =>
  assert.equal(canDeleteProfile(profiles, "p1"), true),
);
check("the last profile can go", () =>
  assert.equal(canDeleteProfile([work], "p1"), true),
);
check("the defaults stay", () =>
  assert.equal(canDeleteProfile(profiles, DEFAULT_PROFILE_ID), false),
);
check("unknown id can't be deleted", () =>
  assert.equal(canDeleteProfile(profiles, "gone"), false),
);

// Deleting the profile in use puts the defaults in use (null); deleting
// another keeps the one in use.
check("deleting the one in use falls back to the defaults", () =>
  assert.equal(profileAfterDelete(profiles, "p1", "p1"), null),
);
check("deleting another keeps the one in use", () =>
  assert.equal(profileAfterDelete(profiles, "p1", "p2"), work),
);
check("with the defaults in use they stay", () =>
  assert.equal(profileAfterDelete(profiles, DEFAULT_PROFILE_ID, "p1"), null),
);

// A name for a new profile: trimmed, not empty, not one already shown
// (ignoring case, the defaults' translated name included).
check("new name is trimmed", () =>
  assert.deepEqual(checkProfileName(profiles, "  Travel ", defaults), {
    kind: "ok",
    name: "Travel",
  }),
);
check("new name can't be blank", () =>
  assert.deepEqual(checkProfileName(profiles, "   ", defaults), {
    kind: "empty",
  }),
);
check("new name taken, ignoring case", () =>
  assert.deepEqual(checkProfileName(profiles, "work", defaults), {
    kind: "taken",
  }),
);
check("new name taken by a name with blanks", () =>
  assert.deepEqual(checkProfileName(profiles, "CODE", defaults), {
    kind: "taken",
  }),
);
check("new name taken by the defaults' name", () =>
  assert.deepEqual(checkProfileName(profiles, "grundeinstellungen", defaults), {
    kind: "taken",
  }),
);

// A new name for a profile: the same name is no change, and the profile's
// own name doesn't count as taken; every profile needs a name.
check("same name is no change", () =>
  assert.deepEqual(checkProfileName(profiles, " Work ", defaults, work), {
    kind: "unchanged",
  }),
);
check("own name in other case is a change", () =>
  assert.deepEqual(checkProfileName(profiles, "WORK", defaults, work), {
    kind: "ok",
    name: "WORK",
  }),
);
check("another profile's name is taken", () =>
  assert.deepEqual(checkProfileName(profiles, "Code", defaults, work), {
    kind: "taken",
  }),
);
check("a profile can't lose its name", () =>
  assert.deepEqual(checkProfileName(profiles, "", defaults, work), {
    kind: "empty",
  }),
);
check("a profile can't take the defaults' name", () =>
  assert.deepEqual(checkProfileName(profiles, defaults, defaults, work), {
    kind: "taken",
  }),
);

if (failures.length > 0) {
  console.error(
    `profiles.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`profiles.test.ts: all ${total} checks passed`);
