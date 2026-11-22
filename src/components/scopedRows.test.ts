// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/scopedRows.test.ts
//
// DanyWhere: every row of a setting has its own profile wrapper
// (`ScopedSetting`), so "+ Profile", the chain and the state line are about
// that row only (qa 2026-11-22_1603, the user's answer to B2). A wrapper
// holds several fields only where one row shows them all. The pages import
// every setting (and through them i18n's Vite-only glob), so they can't be
// loaded here; the checks read their source instead.
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

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

const advanced = read("./settings/advanced/AdvancedSettings.tsx");
const overlay = read("./settings/ShowOverlay.tsx");
const postProcessing = read(
  "./settings/post-processing/PostProcessingSettings.tsx",
);
const context = read(
  "./settings/dictation-context/DictationContextSettings.tsx",
);

check("every file is found", () => {
  assert.ok(advanced && overlay && postProcessing && context);
});

// --- The overlay: style and position are two rows -----------------------
check("the overlay's style has a wrapper of its own", () =>
  assert.match(
    advanced,
    /<ScopedSetting field="overlay_style">\s*<OverlayStyleSetting\b/,
  ),
);
check("the overlay's position has a wrapper of its own", () =>
  assert.match(
    advanced,
    /<ScopedSetting field="overlay_position">\s*<OverlayPositionSetting\b/,
  ),
);
check("each overlay row is a component of its own", () => {
  assert.match(overlay, /export const OverlayStyleSetting\b/);
  assert.match(overlay, /export const OverlayPositionSetting\b/);
});
check("Handy's ShowOverlay still draws both rows", () => {
  const body = overlay.slice(overlay.search(/export const ShowOverlay\b/));
  assert.match(body, /<OverlayStyleSetting\b/);
  assert.match(body, /<OverlayPositionSetting\b/);
});

// --- The context from earlier dictations: four rows ---------------------
const CONTEXT_FIELDS = [
  "context_age_limit_minutes",
  "context_max_entries",
  "context_char_budget",
  "context_excluded_apps",
];
for (const field of CONTEXT_FIELDS) {
  check(`${field} has a wrapper of its own`, () =>
    assert.match(context, new RegExp(`<ScopedSetting field="${field}">`)),
  );
}
// Under the defaults a deviating profile shows its own control, as for any
// small setting, so each row reads its scope's values itself instead of
// getting the page's as props.
check("the context rows read their own values", () =>
  assert.doesNotMatch(context, /settings=\{settings\}/),
);
check(
  "the context rows show a profile's own control under the defaults",
  () => {
    assert.doesNotMatch(context, /textOnly/);
    assert.doesNotMatch(postProcessing, /"context_age_limit_minutes"/);
  },
);

// --- No other wrapper holds the fields of several rows --------------------
/** Every .tsx file under `dir` (a URL of a folder ending in "/"). */
const sources = (dir: URL): string[] =>
  readdirSync(dir, { withFileTypes: true }).flatMap((item) => {
    if (item.isDirectory()) return sources(new URL(`${item.name}/`, dir));
    return item.name.endsWith(".tsx")
      ? [readFileSync(new URL(item.name, dir), "utf8")]
      : [];
  });
const multiFieldWrappers = sources(new URL("./", import.meta.url))
  .flatMap((text) => [
    ...text.matchAll(/<ScopedSetting\s+field=\{\[([^\]]*)\]\}/g),
  ])
  .map((match) =>
    [...match[1].matchAll(/"([a-z_]+)"/g)].map((name) => name[1]).join("+"),
  )
  .sort();
check("only wrappers of one row hold several fields", () =>
  assert.deepEqual(multiFieldWrappers, [
    // One toggle; the own filler words have no control of their own.
    "filler_word_removal_enabled+custom_filler_words",
    // One row: the script's path is a field inside the paste method's row.
    "paste_method+external_script_path",
  ]),
);

// --- The jump from the page "Profile" (B5) -------------------------------
const overview = read("./settings/profiles/ProfileOverview.tsx");
const wrapper = read("./settings/profiles/scope/ScopedSetting.tsx");
check("a row of the page Profile asks to jump to its setting", () =>
  assert.match(overview, /openPage\?\.\(profileId, page, jumpTarget\(/),
);
// "Open in …" under a default jumps there too (qa 2026-11-22_1638).
check("Open in … under a default asks to jump to its setting", () =>
  assert.match(
    wrapper,
    /openProfilePage\(\s*profile\.id,\s*page,\s*openInTarget\(/,
  ),
);
check("Open in … targets the profile's own fields of the setting", () =>
  assert.match(wrapper, /openInTarget\(\s*profile\.id,\s*fields,\s*entry,/),
);
check("a wrapper in a profile lights up when the jump is its", () =>
  assert.match(wrapper, /useJumpHighlight\(/),
);
check("it lights up in the accent colour, by its variables", () =>
  assert.match(
    wrapper,
    /bg-logo-primary\/\d+ inset-ring-2 inset-ring-logo-primary/,
  ),
);
check("it fades only where motion is welcome", () =>
  assert.match(
    wrapper,
    /motion-safe:transition-\[background-color,box-shadow\]/,
  ),
);

if (failures.length > 0) {
  console.error(
    `scopedRows.test.ts: ${failures.length} of ${total} checks failed`,
  );
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`scopedRows.test.ts: all ${total} checks passed`);
