import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import {
  appChoices,
  appLabel,
  appsCaption,
  droppedByOwner,
  hasAssignments,
  ownerOfApp,
  withApp,
  withoutApp,
} from "./profileApps";

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

const standard = { id: "default", name: "", apps: [] as string[] };
const mail = { id: "mail", name: "Mail", apps: ["com.apple.mail"] };
const applications = [
  { id: "com.apple.mail", name: "Mail", running: true },
  { id: "com.apple.Notes", name: "Notes", running: false },
  { id: "com.apple.TextEdit", name: "TextEdit", running: false },
];

check("no assignments", () =>
  assert.equal(hasAssignments([standard, { ...mail, apps: [] }]), false),
);
check("an assignment", () =>
  assert.equal(hasAssignments([standard, mail]), true),
);

check("the app's other profile", () =>
  assert.equal(
    ownerOfApp([standard, mail], "com.apple.mail", "default")?.id,
    "mail",
  ),
);
check("not its own profile", () =>
  assert.equal(ownerOfApp([standard, mail], "com.apple.mail", "mail"), null),
);
check("an unassigned app has no owner", () =>
  assert.equal(
    ownerOfApp([standard, mail], "com.apple.Notes", "default"),
    null,
  ),
);

check("adding appends, trimmed", () =>
  assert.deepEqual(withApp(["com.apple.mail"], " com.apple.Notes "), [
    "com.apple.mail",
    "com.apple.Notes",
  ]),
);
check("adding twice keeps one", () =>
  assert.deepEqual(withApp(["com.apple.mail"], "com.apple.mail"), [
    "com.apple.mail",
  ]),
);
check("adding nothing changes nothing", () =>
  assert.deepEqual(withApp(["com.apple.mail"], "  "), ["com.apple.mail"]),
);
check("removing", () =>
  assert.deepEqual(
    withoutApp(["com.apple.mail", "com.apple.Notes"], "com.apple.mail"),
    ["com.apple.Notes"],
  ),
);

check("choices leave out what's assigned, in order", () =>
  assert.deepEqual(
    appChoices(applications, ["com.apple.Notes"]).map((app) => app.id),
    ["com.apple.mail", "com.apple.TextEdit"],
  ),
);

check("a listed app shows its name", () =>
  assert.equal(appLabel("com.apple.mail", applications), "Mail"),
);
check("an unlisted app shows its id", () =>
  assert.equal(appLabel("Code.exe", applications), "Code.exe"),
);

// An import leaves out the apps another profile here already has.
const notes = { id: "notes", name: "Notes", apps: ["com.apple.Notes"] };
check("dropped apps grouped by their profile", () =>
  assert.deepEqual(
    droppedByOwner(
      ["com.apple.Notes", "com.apple.mail"],
      [standard, mail, notes],
    ).map(({ profile, apps }) => [profile.id, apps]),
    [
      ["mail", ["com.apple.mail"]],
      ["notes", ["com.apple.Notes"]],
    ],
  ),
);
check("nothing dropped, nothing to say", () =>
  assert.deepEqual(droppedByOwner([], [standard, mail]), []),
);
check("a dropped app no profile has any more is left out", () =>
  assert.deepEqual(
    droppedByOwner(["com.apple.TextEdit"], [standard, mail]),
    [],
  ),
);

// ---- a profile without applications (qa 2026-11-22_0320, question 5) -----
// "Used automatically in: no application yet" read as a broken sentence;
// without applications the line is one sentence of its own.
const localesDir = new URL("../../i18n/locales/", import.meta.url);
const appsTexts = (locale: string): Record<string, string> =>
  (
    JSON.parse(
      readFileSync(new URL(`${locale}/translation.json`, localesDir), "utf8"),
    ) as { settings: { profiles: { apps: Record<string, string> } } }
  ).settings.profiles.apps;
const localeNames = readdirSync(localesDir, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();

check("without applications only the sentence of its own shows", () =>
  assert.equal(appsCaption([]), "settings.profiles.apps.none"),
);
check("with applications the label stands before them", () =>
  assert.equal(appsCaption(["com.apple.mail"]), "settings.profiles.apps.label"),
);
check("the sentence, in English and German", () => {
  assert.equal(appsTexts("en").none, "Not assigned to any application yet");
  assert.equal(appsTexts("de").none, "Noch keiner Anwendung zugeordnet");
});
check("in every locale the sentence starts as a sentence", () => {
  // A script with capitals starts it with one; it no longer continues the
  // label in lower case.
  const lower = localeNames.filter((locale) => {
    const first = [...appsTexts(locale).none][0] ?? "";
    return first !== first.toUpperCase();
  });
  assert.deepEqual(lower, []);
});

if (failures.length > 0) {
  console.error(
    `profileApps.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`profileApps.test.ts: all ${total} checks passed`);
