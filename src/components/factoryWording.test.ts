// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/factoryWording.test.ts
//
// DanyWhere: the wiring of qa 2026-11-22_1632, points 4 and 6. Handy's reset
// buttons go through one component, which names the factory setting and
// hides itself in a profile; the device lists and the provider list show
// translated names for stored English ones; the info button, the sound
// preview and the overlay's cancel button take their labels from the
// locales. The components import i18n's Vite-only glob, so they can't be
// loaded here; the checks read their source, as settingsAreas.test.ts does.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

/** A source file next to this one; empty when it doesn't exist. */
const read = (path: string) => {
  const url = new URL(path, import.meta.url);
  return existsSync(url) ? readFileSync(url, "utf8") : "";
};

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

const factoryButton = read("./ui/FactoryResetButton.tsx");

// ---- point 4: one component for Handy's reset buttons ----------------------
check("the factory reset button exists", () => assert.ok(factoryButton));
check("it hides itself by the page's scope", () => {
  assert.match(factoryButton, /useSettingsScope\(\)/);
  assert.match(factoryButton, /showsFactoryReset\(/);
});
check("it names the factory setting", () =>
  assert.match(factoryButton, /t\("common\.resetSetting"/),
);

const RESETTING = {
  slider: "./ui/Slider.tsx",
  shortcut: "./settings/ShortcutInput.tsx",
  microphone: "./settings/MicrophoneSelector.tsx",
  clamshell: "./settings/ClamshellMicrophoneSelector.tsx",
  language: "./settings/LanguageSelector.tsx",
  output: "./settings/OutputDeviceSelector.tsx",
};
for (const [name, path] of Object.entries(RESETTING)) {
  const source = read(path);
  check(`${name}: resets through the factory reset button`, () => {
    assert.ok(source, `${path} not found`);
    assert.match(source, /<FactoryResetButton\b/);
    assert.doesNotMatch(source, /<ResetButton\b/);
    assert.doesNotMatch(source, /common\.resetSetting/);
  });
}

// ---- point 6 no. 2: the default device's name -------------------------------
const DEVICE_LISTS = {
  microphone: "./settings/MicrophoneSelector.tsx",
  clamshell: "./settings/ClamshellMicrophoneSelector.tsx",
  output: "./settings/OutputDeviceSelector.tsx",
};
for (const [name, path] of Object.entries(DEVICE_LISTS)) {
  check(`${name}: the list shows each device by deviceLabel`, () =>
    assert.match(read(path), /label: deviceLabel\(device\.name, t\)/),
  );
}

// ---- point 6 no. 8: the custom provider's name ------------------------------
check("the provider list names each provider by providerName", () =>
  assert.match(
    read("./settings/PostProcessingSettingsApi/usePostProcessProviderState.ts"),
    /label: providerName\(provider, t\)/,
  ),
);
check("the profile import names providers by providerName", () =>
  assert.match(
    read("./settings/profiles/ProfileImportButton.tsx"),
    /import \{[^}]*\bproviderName\b[^}]*\} from "@\/lib\/displayNames"/,
  ),
);

// ---- point 6 nos. 1, 7, 10: labels from the locales -------------------------
check("the info button's label is translated", () => {
  const source = read("./ui/InfoTooltip.tsx");
  assert.doesNotMatch(source, /"More information"/);
  assert.match(source, /t\("common\.moreInformation"\)/);
});
check("the sound preview's tooltip is translated", () => {
  const source = read("./settings/SoundPicker.tsx");
  assert.doesNotMatch(source, /Preview sound theme/);
  assert.match(source, /t\("settings\.debug\.soundTheme\.preview"\)/);
});
check("the overlay's cancel button is translated", () => {
  const source = read("../overlay/RecordingOverlay.tsx");
  assert.doesNotMatch(source, /aria-label="cancel"/);
  assert.match(source, /aria-label=\{t\("overlay\.cancel"\)\}/);
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`factoryWording: all ${total} checks passed`);
