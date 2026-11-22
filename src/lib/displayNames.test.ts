// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/displayNames.test.ts
//
// DanyWhere: Handy's reset buttons say that they go back to the factory
// setting and are hidden in a profile (qa 2026-11-22_1632, point 4); values
// stored in English are shown translated, the stored value stays (point 6:
// the default device, the custom provider; the info buttons, the sound
// preview and the overlay's cancel button are labels). With i18next itself
// and the real locale files.
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import i18next from "i18next";
import {
  deviceLabel,
  providerName,
  showsFactoryReset,
  type Translate,
} from "./displayNames";
import { DEFAULTS_SCOPE } from "./profileScope";

let total = 0;
const failures: string[] = [];
const check = (name: string, body: () => void) => {
  total += 1;
  try {
    body();
  } catch (error) {
    failures.push(
      `${name}: ${(error as Error).message.split("\n").slice(0, 6).join(" ")}`,
    );
  }
};

const localesDir = new URL("../i18n/locales/", import.meta.url);
const locales = readdirSync(localesDir, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();
const files = Object.fromEntries(
  locales.map((code) => [
    code,
    JSON.parse(
      readFileSync(new URL(`${code}/translation.json`, localesDir), "utf8"),
    ) as Record<string, unknown>,
  ]),
);

const i18n = i18next.createInstance();
await i18n.init({
  lng: "de",
  fallbackLng: false,
  resources: Object.fromEntries(
    locales.map((code) => [code, { translation: files[code] }]),
  ),
  interpolation: { escapeValue: false },
});
const tFor = (lng: string): Translate => {
  const fixed = i18n.getFixedT(lng);
  return (key, options) => String(fixed(key, options ?? {}));
};
const de = tFor("de");
const en = tFor("en");

/** The text at a dotted key of a locale file, or undefined. */
const textAt = (code: string, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>(
      (node, part) =>
        node && typeof node === "object"
          ? (node as Record<string, unknown>)[part]
          : undefined,
      files[code],
    );

// ---- point 4: the factory setting ------------------------------------------
check("the factory reset shows under the defaults", () =>
  assert.equal(showsFactoryReset(DEFAULTS_SCOPE), true),
);
check("the factory reset shows outside a scope (app settings)", () =>
  assert.equal(showsFactoryReset(null), true),
);
check("the factory reset is hidden in a profile", () =>
  assert.equal(showsFactoryReset({ kind: "profile", id: "arbeit" }), false),
);
check("German: the reset goes back to the factory setting", () =>
  assert.equal(
    de("common.resetSetting", { setting: "Mikrofon" }),
    "Mikrofon auf die Werkseinstellung zurücksetzen",
  ),
);
check("English: the reset goes back to the factory setting", () =>
  assert.equal(
    en("common.resetSetting", { setting: "Microphone" }),
    "Reset Microphone to the factory setting",
  ),
);
check("no locale still says the old 'default value'", () => {
  const old: Record<string, string> = {
    de: "{{setting}} auf den Standardwert zurücksetzen",
    en: "Reset {{setting}} to default",
    fr: "Rétablir la valeur par défaut de {{setting}}",
    es: "Restablecer {{setting}} al valor predeterminado",
  };
  const still = Object.entries(old)
    .filter(([code, text]) => textAt(code, "common.resetSetting") === text)
    .map(([code]) => code);
  assert.deepEqual(still, []);
});

// ---- point 6 no. 2: the system's default device ----------------------------
check("the default device is shown as the system default", () => {
  assert.equal(deviceLabel("Default", de), "Systemstandard");
  assert.equal(deviceLabel("default", de), "Systemstandard");
  assert.equal(deviceLabel("Default", en), "System default");
});
check("a real device keeps its name", () => {
  assert.equal(deviceLabel("MacBook Pro-Mikrofon", de), "MacBook Pro-Mikrofon");
  assert.equal(deviceLabel("Defaulter USB", de), "Defaulter USB");
});

// ---- point 6 no. 8: the custom provider ------------------------------------
check("the custom provider has a German name", () =>
  assert.equal(
    providerName({ id: "custom", label: "Custom" }, de),
    "Eigener Anbieter",
  ),
);
check("the custom provider keeps its English name in English", () =>
  assert.equal(providerName({ id: "custom", label: "Custom" }, en), "Custom"),
);
check("another provider keeps its stored label", () =>
  assert.equal(
    providerName({ id: "anthropic", label: "Anthropic" }, de),
    "Anthropic",
  ),
);

// ---- point 6 nos. 1, 7, 10: labels -----------------------------------------
check("German labels of the info button, the preview and cancel", () => {
  assert.equal(de("common.moreInformation"), "Weitere Informationen");
  assert.equal(
    de("settings.debug.soundTheme.preview"),
    "Sound-Thema anhören (spielt Start, Stopp und dann den Ton, der ein Diktat beendet)",
  );
  assert.equal(de("overlay.cancel"), "Abbrechen");
});
check("English labels keep Handy's words", () => {
  assert.equal(en("common.moreInformation"), "More information");
  assert.equal(
    en("settings.debug.soundTheme.preview"),
    "Preview sound theme (plays start, stop, then the sound that ends a dictation)",
  );
  assert.equal(en("overlay.cancel"), "Cancel");
});

// ---- every locale has every new text, translated ---------------------------
const NEW_KEYS = [
  "common.moreInformation",
  "settings.sound.systemDefault",
  "settings.debug.soundTheme.preview",
  "settings.postProcessing.api.provider.custom",
  "overlay.cancel",
];
check("every locale has every new text", () => {
  const missing = locales.flatMap((code) =>
    NEW_KEYS.filter((key) => {
      const text = textAt(code, key);
      return typeof text !== "string" || !text.trim();
    }).map((key) => `${code}:${key}`),
  );
  assert.deepEqual(missing, []);
});
check("the cancel button says what the tray's cancel says", () => {
  const differ = locales.filter(
    (code) => textAt(code, "overlay.cancel") !== textAt(code, "tray.cancel"),
  );
  assert.deepEqual(differ, []);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`displayNames: all ${total} checks passed`);
