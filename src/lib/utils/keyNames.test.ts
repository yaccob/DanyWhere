// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/utils/keyNames.test.ts
//
// DanyWhere: key combinations shown as the system names them (user,
// 2026-11-22, design question 3a and point 6 no. 3 and 4 after ideas 1
// to 3). On macOS the symbols on the keys (⌃⌥⇧⌘, ↩), written together as
// the menus do; on Windows and Linux the names on the keys, translated and
// joined with "+" ("Strg+Umschalt+V"). A side shows only where it is kept.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import de from "../../i18n/locales/de/translation.json";
import en from "../../i18n/locales/en/translation.json";
import {
  clipboardPasteKeys,
  ENGLISH_KEY_NAMES,
  formatKeyCombination,
  type KeyNameT,
  type OSType,
} from "./keyboard";

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

/** `t` over a locale file, as i18next does it: the key when it is missing. */
const translator =
  (locale: unknown): KeyNameT =>
  (key, options = {}) => {
    const text = key
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        locale,
      );
    if (typeof text !== "string") return key;
    return text.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
      String(options[name] ?? ""),
    );
  };
const deT = translator(de);
const enT = translator(en);

const shown = (keys: string, os: OSType, t?: KeyNameT) =>
  formatKeyCombination(keys, os, t);

// ---- macOS -------------------------------------------------------------

check("macOS writes the modifier symbols together", () => {
  assert.equal(shown("ctrl+option+d", "macos", deT), "⌃⌥D");
  assert.equal(shown("ctrl+option+shift+d", "macos", deT), "⌃⌥⇧D");
  assert.equal(shown("command+k", "macos", enT), "⌘K");
});

check("macOS orders the modifiers as its menus do", () => {
  assert.equal(shown("command+shift+k", "macos", deT), "⇧⌘K");
  assert.equal(shown("shift+ctrl+x", "macos", deT), "⌃⇧X");
});

check("macOS shows Return, Tab, Delete and the arrows as symbols", () => {
  assert.equal(shown("ctrl+return", "macos", deT), "⌃↩");
  assert.equal(shown("ctrl+enter", "macos", deT), "⌃↩");
  assert.equal(shown("option+tab", "macos", deT), "⌥⇥");
  assert.equal(shown("command+delete", "macos", deT), "⌘⌫");
  assert.equal(shown("ctrl+up", "macos", deT), "⌃↑");
  assert.equal(shown("command+f5", "macos", deT), "⌘F5");
});

check("macOS names the space bar in the language of the interface", () => {
  assert.equal(shown("option+space", "macos", deT), "⌥ Leertaste");
  assert.equal(shown("option+space", "macos", enT), "⌥ Space");
  assert.equal(shown("fn+space", "macos", deT), "fn Leertaste");
});

check("macOS writes Escape as on the key", () => {
  assert.equal(shown("escape", "macos", deT), "esc");
  assert.equal(shown("f13", "macos", deT), "F13");
});

check("macOS shows a kept side in words", () => {
  assert.equal(shown("option_right", "macos", deT), "⌥ rechts");
  assert.equal(shown("option_right", "macos", enT), "Right ⌥");
  assert.equal(shown("ctrl_left+option_left", "macos", deT), "⌃ links ⌥ links");
  assert.equal(shown("ctrl_left+return", "macos", deT), "⌃ links ↩");
});

// ---- Windows and Linux -------------------------------------------------

check("Windows and Linux name the keys in the interface's language", () => {
  assert.equal(shown("ctrl+shift+v", "windows", deT), "Strg+Umschalt+V");
  assert.equal(shown("ctrl+shift+v", "linux", enT), "Ctrl+Shift+V");
  assert.equal(shown("ctrl+space", "linux", deT), "Strg+Leertaste");
  assert.equal(shown("ctrl+return", "windows", deT), "Strg+Eingabe");
  assert.equal(shown("ctrl+enter", "linux", deT), "Strg+Eingabe");
  assert.equal(shown("alt+shift+l", "windows", deT), "Alt+Umschalt+L");
});

check("Windows and Linux order the modifiers Ctrl, Alt, Shift, Win", () => {
  assert.equal(shown("shift+ctrl+x", "windows", enT), "Ctrl+Shift+X");
  assert.equal(shown("super+shift+k", "linux", enT), "Shift+Super+K");
  assert.equal(shown("super+k", "windows", enT), "Win+K");
});

check("Windows and Linux name the editing keys as printed on them", () => {
  assert.equal(shown("escape", "windows", deT), "Esc");
  assert.equal(shown("ctrl+pageup", "windows", deT), "Strg+Bild auf");
  assert.equal(shown("ctrl+forwarddelete", "windows", deT), "Strg+Entf");
  // handy-keys (Windows) calls Backspace "delete", Tauri (Linux) the Delete key.
  assert.equal(shown("ctrl+delete", "windows", deT), "Strg+Rücktaste");
  assert.equal(shown("ctrl+delete", "linux", deT), "Strg+Entf");
  assert.equal(shown("ctrl+backspace", "linux", deT), "Strg+Rücktaste");
  assert.equal(shown("ctrl+home", "windows", deT), "Strg+Pos1");
});

check("a kept side shows in words off macOS too", () => {
  assert.equal(shown("alt_left+q", "windows", deT), "Alt links+Q");
  assert.equal(shown("alt_left+q", "windows", enT), "Left Alt+Q");
});

check("keypad keys say so", () => {
  assert.equal(shown("keypad1", "windows", deT), "Ziffernblock 1");
  assert.equal(shown("numpad 1", "linux", deT), "Ziffernblock 1");
  assert.equal(shown("keypad1", "macos", enT), "Keypad 1");
});

check("the English names without a translation are the locale's", () => {
  assert.deepEqual(ENGLISH_KEY_NAMES, (en as Record<string, unknown>).keyNames);
});

check("without a translation the names are English", () => {
  assert.equal(shown("ctrl+shift+v", "windows"), "Ctrl+Shift+V");
  assert.equal(shown("option+space", "macos"), "⌥ Space");
  assert.equal(shown("pageup", "linux"), "Page Up");
});

// ---- Paste method ------------------------------------------------------

check("the clipboard paste names its keys like a shortcut", () => {
  assert.equal(shown(clipboardPasteKeys("macos"), "macos", deT), "⌘V");
  assert.equal(shown(clipboardPasteKeys("windows"), "windows", deT), "Strg+V");
  assert.equal(shown(clipboardPasteKeys("linux"), "linux", enT), "Ctrl+V");
});

check("every locale's clipboard paste takes the keys whole", () => {
  const dir = new URL("../../i18n/locales/", import.meta.url);
  for (const locale of readdirSync(dir)) {
    const file = JSON.parse(
      readFileSync(new URL(`${locale}/translation.json`, dir), "utf8"),
    );
    const text: string =
      file.settings.advanced.pasteMethod.options.clipboard ?? "";
    assert.ok(
      text.includes("{{keys}}") && !text.includes("{{modifier}}"),
      `${locale}: ${text}`,
    );
  }
});

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`keyNames: all ${total} checks passed`);
