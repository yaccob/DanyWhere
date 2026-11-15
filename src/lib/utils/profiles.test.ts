import assert from "node:assert/strict";
import {
  DEFAULT_PROFILE_ID,
  activeProfile,
  canDeleteProfile,
  checkProfileName,
  profileAfterDelete,
  profileDisplayName,
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

const standard = { id: DEFAULT_PROFILE_ID, name: "" };
const work = { id: "p1", name: "Work" };
const code = { id: "p2", name: "  Code  " };
const profiles = [standard, work, code];

// The default profile's stored name is empty and shows translated until it
// is renamed; any other name shows as stored, without surrounding blanks.
check("default id", () => assert.equal(DEFAULT_PROFILE_ID, "default"));
check("empty name shows translated", () =>
  assert.equal(profileDisplayName(standard, "Standard"), "Standard"),
);
check("blank name shows translated", () =>
  assert.equal(
    profileDisplayName({ ...standard, name: "  " }, "Default"),
    "Default",
  ),
);
check("renamed default shows its name", () =>
  assert.equal(
    profileDisplayName({ ...standard, name: "Home" }, "Standard"),
    "Home",
  ),
);
check("other name shows as stored", () =>
  assert.equal(profileDisplayName(work, "Standard"), "Work"),
);
check("name shows trimmed", () =>
  assert.equal(profileDisplayName(code, "Standard"), "Code"),
);

// The profile in use: the one the id names, else the first, as the backend
// falls back; none without profiles.
check("active by id", () => assert.equal(activeProfile(profiles, "p1"), work));
check("unknown id falls back to the first", () =>
  assert.equal(activeProfile(profiles, "gone"), standard),
);
check("null id falls back to the first", () =>
  assert.equal(activeProfile(profiles, null), standard),
);
check("missing id falls back to the first", () =>
  assert.equal(activeProfile(profiles, undefined), standard),
);
check("no profiles, no active one", () =>
  assert.equal(activeProfile([], "p1"), null),
);

// The last profile can't be deleted; an unknown id can't either.
check("delete one of several", () =>
  assert.equal(canDeleteProfile(profiles, "p1"), true),
);
check("delete the default one of several", () =>
  assert.equal(canDeleteProfile(profiles, DEFAULT_PROFILE_ID), true),
);
check("the last profile stays", () =>
  assert.equal(canDeleteProfile([work], "p1"), false),
);
check("unknown id can't be deleted", () =>
  assert.equal(canDeleteProfile(profiles, "gone"), false),
);
check("nothing to delete", () =>
  assert.equal(canDeleteProfile([], "p1"), false),
);

// Deleting the active profile switches to the first other one first;
// deleting another keeps the active one.
check("deleting the active one picks the first other", () =>
  assert.equal(profileAfterDelete(profiles, "p1", "p1"), standard),
);
check("deleting the active first one picks the second", () =>
  assert.equal(
    profileAfterDelete(profiles, DEFAULT_PROFILE_ID, DEFAULT_PROFILE_ID),
    work,
  ),
);
check("deleting another keeps the active one", () =>
  assert.equal(profileAfterDelete(profiles, "p1", "p2"), work),
);
check("unknown active id means the first is active", () =>
  assert.equal(profileAfterDelete(profiles, "gone", DEFAULT_PROFILE_ID), work),
);
check("nothing is left after the last", () =>
  assert.equal(profileAfterDelete([work], "p1", "p1"), null),
);

// A name for a duplicate: trimmed, not empty, not one already shown
// (ignoring case, the translated default name included).
check("new name is trimmed", () =>
  assert.deepEqual(checkProfileName(profiles, "  Travel ", "Standard"), {
    kind: "ok",
    name: "Travel",
  }),
);
check("new name can't be blank", () =>
  assert.deepEqual(checkProfileName(profiles, "   ", "Standard"), {
    kind: "empty",
  }),
);
check("new name taken, ignoring case", () =>
  assert.deepEqual(checkProfileName(profiles, "work", "Standard"), {
    kind: "taken",
  }),
);
check("new name taken by a name with blanks", () =>
  assert.deepEqual(checkProfileName(profiles, "CODE", "Standard"), {
    kind: "taken",
  }),
);
check("new name taken by the translated default name", () =>
  assert.deepEqual(checkProfileName(profiles, "standard", "Standard"), {
    kind: "taken",
  }),
);

// A new name for a profile: the same name is no change, and the profile's
// own name doesn't count as taken.
check("same name is no change", () =>
  assert.deepEqual(checkProfileName(profiles, " Work ", "Standard", work), {
    kind: "unchanged",
  }),
);
check("own name in other case is a change", () =>
  assert.deepEqual(checkProfileName(profiles, "WORK", "Standard", work), {
    kind: "ok",
    name: "WORK",
  }),
);
check("another profile's name is taken", () =>
  assert.deepEqual(checkProfileName(profiles, "Code", "Standard", work), {
    kind: "taken",
  }),
);
check("only the default profile may have no name", () =>
  assert.deepEqual(checkProfileName(profiles, "", "Standard", work), {
    kind: "empty",
  }),
);

// Only the default profile may get its empty name back, which shows the
// translated name again; it already has it when its name is empty.
const home = { id: DEFAULT_PROFILE_ID, name: "Home" };
check("default profile gets its empty name back", () =>
  assert.deepEqual(checkProfileName([home, work], "  ", "Standard", home), {
    kind: "ok",
    name: "",
  }),
);
check("default profile already has no name", () =>
  assert.deepEqual(checkProfileName(profiles, "", "Standard", standard), {
    kind: "unchanged",
  }),
);
// Typing the translated name fixes it, so it no longer follows the language.
check("typing the translated name stores it", () =>
  assert.deepEqual(
    checkProfileName(profiles, "Standard", "Standard", standard),
    { kind: "ok", name: "Standard" },
  ),
);

if (failures.length > 0) {
  console.error(
    `profiles.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`profiles.test.ts: all ${total} checks passed`);
