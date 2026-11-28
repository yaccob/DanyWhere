// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/manual.test.ts
//
// DanyWhere: the user manual on its page in the settings window. Which
// address the frame shows for the interface language and a chapter, which
// pages it falls back to when the build lacks one, and what a click on a
// link inside it does: stay in the manual, open in the default browser, or
// nothing.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  MANUAL_PAGES,
  isExternal,
  manualCandidates,
  manualLink,
  manualUrl,
} from "./manual";

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

const CHAPTER = MANUAL_PAGES.startAndEndShortcuts;

// --- The address: languages ---------------------------------------------------
check("the chapter the End Shortcuts link to", () =>
  assert.equal(CHAPTER, "how-it-works/start-and-end-shortcuts/"),
);
check("English is the root locale", () => {
  assert.equal(manualUrl("en"), "/manual/");
  assert.equal(
    manualUrl("en", CHAPTER),
    "/manual/how-it-works/start-and-end-shortcuts/",
  );
});
check("German, Spanish and French have a prefix of their own", () => {
  assert.equal(manualUrl("de"), "/manual/de/");
  assert.equal(
    manualUrl("de", CHAPTER),
    "/manual/de/how-it-works/start-and-end-shortcuts/",
  );
  assert.equal(
    manualUrl("es", CHAPTER),
    "/manual/es/how-it-works/start-and-end-shortcuts/",
  );
  assert.equal(
    manualUrl("fr", CHAPTER),
    "/manual/fr/how-it-works/start-and-end-shortcuts/",
  );
});
check("a regional locale takes its language's manual", () => {
  assert.equal(manualUrl("de-AT"), "/manual/de/");
  assert.equal(manualUrl("de_CH"), "/manual/de/");
  assert.equal(manualUrl("es-419"), "/manual/es/");
  assert.equal(manualUrl("fr-CA"), "/manual/fr/");
  assert.equal(manualUrl("DE"), "/manual/de/");
  assert.equal(manualUrl("en-GB"), "/manual/");
});
check("every other language gets English", () => {
  for (const language of ["ja", "zh-TW", "pt-BR", "it", "ru", "", "dex", "d"]) {
    assert.equal(
      manualUrl(language, CHAPTER),
      "/manual/how-it-works/start-and-end-shortcuts/",
      JSON.stringify(language),
    );
  }
});

// --- The address: pages -------------------------------------------------------
check("no page or an empty one is the home page", () => {
  assert.equal(manualUrl("de"), "/manual/de/");
  assert.equal(manualUrl("de", null), "/manual/de/");
  assert.equal(manualUrl("de", ""), "/manual/de/");
});
check("a page gets its trailing slash", () =>
  assert.equal(
    manualUrl("en", "how-it-works/start-and-end-shortcuts"),
    "/manual/how-it-works/start-and-end-shortcuts/",
  ),
);
check("an anchor is kept, after the slash", () => {
  assert.equal(
    manualUrl("fr", "how-it-works/dictation-from-start-to-end/#the-end"),
    "/manual/fr/how-it-works/dictation-from-start-to-end/#the-end",
  );
  assert.equal(
    manualUrl("en", "how-it-works/start-and-end-shortcuts#end-2"),
    "/manual/how-it-works/start-and-end-shortcuts/#end-2",
  );
  assert.equal(manualUrl("de", "#intro"), "/manual/de/#intro");
});
check("an invalid page opens the home page", () => {
  for (const page of [
    "../secret/",
    "how-it-works/../../etc/passwd",
    "./how-it-works/",
    "/how-it-works/",
    "//evil.example/",
    "how-it-works//start/",
    "https://evil.example/",
    "javascript:alert(1)",
    "file:///etc/passwd",
    "How-It-Works/",
    "how it works/",
    "how-it-works/?x=1",
    "%2e%2e/",
    "how-it-works\\start",
    "einführung/",
    "how-it-works/#",
    "how-it-works/#a#b",
    "how-it-works/#Anchor",
    "how-it-works/#an chor",
    "how-it-works/#a/b",
  ]) {
    assert.equal(manualUrl("de", page), "/manual/de/", JSON.stringify(page));
  }
});

// --- What the frame falls back to ---------------------------------------------
check("a chapter falls back to its language's home page, then English", () =>
  assert.deepEqual(manualCandidates("de", CHAPTER), [
    "/manual/de/how-it-works/start-and-end-shortcuts/",
    "/manual/de/",
    "/manual/",
  ]),
);
check("an anchor stays on the first try only", () =>
  assert.deepEqual(manualCandidates("fr-CA", "how-it-works/x/#a"), [
    "/manual/fr/how-it-works/x/#a",
    "/manual/fr/",
    "/manual/",
  ]),
);
check("no page is tried twice", () => {
  assert.deepEqual(manualCandidates("de"), ["/manual/de/", "/manual/"]);
  assert.deepEqual(manualCandidates("en", CHAPTER), [
    "/manual/how-it-works/start-and-end-shortcuts/",
    "/manual/",
  ]);
  assert.deepEqual(manualCandidates("en"), ["/manual/"]);
  assert.deepEqual(manualCandidates("ja", "../x/"), ["/manual/"]);
});

// --- Links inside the frame ---------------------------------------------------
const MAC = "tauri://localhost/";
const WINDOWS = "http://tauri.localhost/";
const DEV = "http://localhost:1420/";

check("a page of the manual stays in the frame", () => {
  for (const link of [
    "tauri://localhost/manual/",
    "tauri://localhost/manual",
    "tauri://localhost/manual/de/how-it-works/start-and-end-shortcuts/",
    "tauri://localhost/manual/how-it-works/start-and-end-shortcuts/#end",
    "tauri://localhost/manual/_astro/index.css",
  ]) {
    assert.equal(manualLink(link, MAC), "manual", link);
  }
  assert.equal(
    manualLink("http://tauri.localhost/manual/fr/", WINDOWS),
    "manual",
  );
  assert.equal(manualLink("http://localhost:1420/manual/es/", DEV), "manual");
});
check("the app's origin may be given with or without a path", () => {
  assert.equal(
    manualLink("tauri://localhost/manual/de/", "tauri://localhost"),
    "manual",
  );
  assert.equal(
    manualLink("tauri://localhost/manual/de/", "tauri://localhost/index.html"),
    "manual",
  );
});
check("the app's own pages outside the manual stay blocked", () => {
  for (const link of [
    "tauri://localhost/",
    "tauri://localhost/index.html",
    "tauri://localhost/manualx/",
    "tauri://localhost/manual/../index.html",
  ]) {
    assert.equal(manualLink(link, MAC), "blocked", link);
  }
  assert.equal(manualLink("http://tauri.localhost/", WINDOWS), "blocked");
  assert.equal(manualLink("http://localhost:1420/", DEV), "blocked");
});
check("links leaving the app open in the default browser", () => {
  for (const link of [
    "https://github.com/yaccob/DanyWhere",
    "http://example.com/manual/",
    "mailto:someone@example.com",
    // Another platform's origin is not this app's.
    "http://tauri.localhost/manual/",
    // A look-alike host is somewhere else.
    "https://tauri.localhost.example.com/manual/",
    // The dev server is the app only while it serves the app.
    "http://localhost:1420/manual/",
  ]) {
    assert.equal(manualLink(link, MAC), "external", link);
  }
  assert.equal(manualLink("http://localhost:1421/manual/", DEV), "external");
});
check("another platform's custom scheme is blocked", () =>
  assert.equal(manualLink("tauri://localhost/manual/", WINDOWS), "blocked"),
);
check("other schemes and what isn't an address are blocked", () => {
  for (const link of [
    "javascript:alert(1)",
    "file:///etc/passwd",
    "data:text/html,hi",
    "ftp://example.com/",
    "tel:+431234",
    "about:blank",
    "about:config",
    "asset://localhost/etc/passwd",
    "ipc://localhost/open_manual",
    "",
    "/manual/de/",
    "not an address",
  ]) {
    assert.equal(manualLink(link, MAC), "blocked", JSON.stringify(link));
  }
});
check("isExternal is true exactly for the links the browser opens", () => {
  assert.equal(isExternal("https://github.com/cjpais/Handy", MAC), true);
  assert.equal(isExternal("mailto:someone@example.com", MAC), true);
  assert.equal(isExternal("http://localhost:1420/manual/", MAC), true);
  assert.equal(isExternal("tauri://localhost/manual/de/", MAC), false);
  assert.equal(isExternal("tauri://localhost/", MAC), false);
  assert.equal(isExternal("javascript:alert(1)", MAC), false);
  assert.equal(isExternal("http://localhost:1420/manual/", DEV), false);
});

// --- The chapters the app links to exist, in every language -----------------
const DOCS = new URL("../../docs/manual/src/content/docs/", import.meta.url);
for (const [name, page] of Object.entries(MANUAL_PAGES)) {
  check(`MANUAL_PAGES.${name} is a chapter in en, de, es and fr`, () => {
    for (const language of ["", "de/", "es/", "fr/"]) {
      const file = new URL(`${language}${page.replace(/\/$/, "")}.mdx`, DOCS);
      assert.ok(existsSync(file), fileURLToPath(file));
    }
  });
}

if (failures.length > 0) {
  console.error(`manual.test.ts: ${failures.length} of ${total} checks failed`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`manual.test.ts: all ${total} checks passed`);
