// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/valueLabels.test.ts
//
// DanyWhere: the texts of a profile's values on the page "Profile", in the
// state lines ("Default: …"), in the quiet bar and under the defaults (qa
// 2026-11-19_1711 P4, P5, P7, P8), with i18next itself and the real locale
// files: a count says its number, an entry is called by its name (its own,
// or the defaults' for one a profile switched off), a shortcut shows its
// keys, a choice its label (qa 2026-11-22_0107, errors 1 and 2).
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import i18next from "i18next";
import { formatKeyCombination } from "./utils/keyboard";
import type { FieldKind, LayeredProfile } from "./profileScope";
import {
  DEFAULT_DEVICE,
  defaultText,
  entryName,
  ownValueLabel,
  ownValueRow,
  valueLabel,
  type LabelContext,
  type Translate,
} from "./valueLabels";

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
const resources = Object.fromEntries(
  locales.map((code) => [
    code,
    {
      translation: JSON.parse(
        readFileSync(new URL(`${code}/translation.json`, localesDir), "utf8"),
      ) as Record<string, unknown>,
    },
  ]),
);

const i18n = i18next.createInstance();
await i18n.init({
  lng: "de",
  fallbackLng: "en",
  resources,
  interpolation: { escapeValue: false },
});

const translatorFor = (lng: string): Translate => {
  const fixed = i18n.getFixedT(lng);
  return (key, options) => String(fixed(key, options ?? {}));
};

const KINDS: Record<string, FieldKind> = {
  word_lists: { kind: "list", key: "id" },
  end_keys: { kind: "list", key: "id" },
  post_process_providers: { kind: "list", key: "id" },
  bindings: { kind: "map" },
  post_process_models: { kind: "map" },
  post_process_efforts: { kind: "map" },
  post_process_api_keys: { kind: "map" },
};

const binding = (id: string, keys: string) => ({
  id,
  name: id,
  description: "",
  default_binding: keys,
  current_binding: keys,
});

const plain = {
  id: "plain",
  name: "",
  shortcut: "",
  append_text: "\\s",
  post_process: false,
  send_context: false,
};
const postProcess = { ...plain, id: "post_process", post_process: true };
const ownEnd = {
  ...plain,
  id: "end_1795307930035",
  name: "PEnde",
  shortcut: "ctrl+option+j",
};

const defaults: Record<string, unknown> = {
  bindings: {
    transcribe: binding("transcribe", "ctrl+option+d"),
    transcribe_with_post_process: binding(
      "transcribe_with_post_process",
      "ctrl+option+shift+d",
    ),
  },
  end_keys: [plain, postProcess],
  default_ends: { transcribe: "plain", transcribe_with_post_process: "plain" },
  word_lists: [{ id: "word_list_1795306966443", name: "GListe", words: [] }],
  active_word_list_id: "word_list_1795306966443",
  post_process_prompt_order: [
    { id: "p1", active: true },
    { id: "p2", active: false },
  ],
  post_process_efforts: { anthropic: "xhigh" },
  post_process_api_keys: { anthropic: "sk-secret" },
  selected_microphone: null,
  overlay_style: "minimal",
  overlay_position: "bottom",
  paste_method: "ctrl_v",
};

const neutest: LayeredProfile = {
  id: "neutest",
  name: "Neutest",
  values: {
    ...defaults,
    bindings: {
      ...(defaults.bindings as Record<string, unknown>),
      transcribe: binding("transcribe", "ctrl+option+e"),
    },
    end_keys: [plain, postProcess, ownEnd],
    word_lists: [{ id: "word_list_1795307006713", name: "PListe", words: [] }],
    overlay_position: "top",
    selected_microphone: "MacBook Pro-Mikrofon",
    paste_method: "direct",
    post_process_prompt_order: [
      { id: "p1", active: true },
      { id: "p2", active: true },
    ],
  },
  own: ["overlay_position", "paste_method", "selected_microphone"],
  own_entries: {
    bindings: { own: ["transcribe"], off: [] },
    end_keys: { own: ["end_1795307930035"], off: [] },
    word_lists: {
      own: ["word_list_1795307006713"],
      off: ["word_list_1795306966443"],
    },
  },
};

const contextFor = (lng: string): LabelContext => ({
  t: translatorFor(lng),
  exists: (key) => i18n.exists(key, { lng }),
  kindOf: (field) => KINDS[field] ?? { kind: "value" },
  defaults,
  prompts: [
    { id: "p1", name: "Diktat (DE/EN)" },
    { id: "p2", name: "Improve Transcriptions" },
  ],
  formatShortcut: (keys) => formatKeyCombination(keys, "macos"),
  optionParams: { modifier: "Cmd" },
});

const de = contextFor("de");
const en = contextFor("en");

// ---- error 1: placeholders, counts, readable values ------------------------
check("a list of objects says how many, in German", () =>
  assert.equal(
    valueLabel(de, "word_lists", [{ id: "a", name: "A" }]),
    "Einträge: 1",
  ),
);
check("no locale leaves a placeholder in a count", () => {
  const left = locales.filter((lng) =>
    valueLabel(contextFor(lng), "word_lists", [
      { id: "a", name: "A" },
      { id: "b", name: "B" },
    ]).includes("{{"),
  );
  assert.deepEqual(left, []);
});
check("the prompt order names the active prompts", () => {
  assert.equal(
    valueLabel(
      de,
      "post_process_prompt_order",
      defaults.post_process_prompt_order,
    ),
    "aktiv: Diktat (DE/EN)",
  );
  assert.equal(
    valueLabel(en, "post_process_prompt_order", [{ id: "p2", active: false }]),
    "no prompt active",
  );
});
check("a shortcut shows its keys", () =>
  assert.equal(
    valueLabel(de, "bindings", binding("transcribe", "ctrl+option+d")),
    formatKeyCombination("ctrl+option+d", "macos"),
  ),
);
check("the default ends name the start shortcuts and their ends", () =>
  assert.equal(
    valueLabel(de, "default_ends", defaults.default_ends, defaults),
    "Transkriptions-Tastenkürzel: Normal · Nachbearbeitungs-Tastenkürzel: Normal",
  ),
);

// ---- error 2: entries by their names ---------------------------------------
check("an own end shortcut is called by its name", () =>
  assert.equal(
    ownValueLabel(
      de,
      { field: "end_keys", entry: "end_1795307930035" },
      neutest.values,
    ),
    "Endtasten: PEnde",
  ),
);
check("an own word list is called by its name", () =>
  assert.equal(
    ownValueLabel(
      de,
      { field: "word_lists", entry: "word_list_1795307006713" },
      neutest.values,
    ),
    "Zusätzliche Listen: PListe",
  ),
);
check("a switched-off list takes its name from the defaults", () => {
  const off = {
    field: "word_lists",
    entry: "word_list_1795306966443",
    off: true,
  };
  assert.equal(
    ownValueLabel(de, off, neutest.values),
    "Zusätzliche Listen: GListe",
  );
  assert.equal(
    ownValueRow(de, off, neutest.values),
    "Zusätzliche Listen: GListe (aus)",
  );
});
check("the bar names an entry the profile no longer has", () =>
  assert.equal(
    entryName(
      de,
      "word_lists",
      "word_list_1795306966443",
      undefined,
      neutest.values,
    ),
    "GListe",
  ),
);
check("the active list is called by its name", () =>
  assert.equal(
    valueLabel(de, "active_word_list_id", "word_list_1795306966443"),
    "GListe",
  ),
);
check("an own shortcut's row on the page Profile shows its keys", () =>
  assert.equal(
    ownValueRow(de, { field: "bindings", entry: "transcribe" }, neutest.values),
    `Tastenkombinationen: Transkriptions-Tastenkürzel: ${formatKeyCombination("ctrl+option+e", "macos")}`,
  ),
);
check("an own end shortcut's row names it without a count", () =>
  assert.equal(
    ownValueRow(
      de,
      { field: "end_keys", entry: "end_1795307930035" },
      neutest.values,
    ),
    "Endtasten: PEnde",
  ),
);

// ---- wording: choices by the label their control shows ---------------------
check("the effort shows its level's label", () => {
  assert.equal(valueLabel(de, "post_process_efforts", "xhigh"), "Sehr hoch");
  assert.equal(
    valueLabel(de, "post_process_efforts", undefined),
    "Standard des Modells",
  );
});
check("the default microphone is called as the list calls it", () => {
  assert.equal(valueLabel(de, "selected_microphone", null), DEFAULT_DEVICE);
  assert.equal(
    valueLabel(de, "selected_microphone", "default"),
    DEFAULT_DEVICE,
  );
  assert.equal(
    valueLabel(de, "selected_microphone", "MacBook Pro-Mikrofon"),
    "MacBook Pro-Mikrofon",
  );
});
check("the paste method shows its option's label", () =>
  assert.equal(valueLabel(en, "paste_method", "ctrl_v"), "Clipboard (Cmd+V)"),
);
check("an API key is never shown", () =>
  assert.equal(
    valueLabel(de, "post_process_api_keys", "sk-secret").includes("sk-"),
    false,
  ),
);

// ---- "Default: …" under a setting of several fields ------------------------
check("one field: the default's value", () =>
  assert.equal(
    defaultText(de, ["selected_microphone"], undefined, neutest),
    DEFAULT_DEVICE,
  ),
);
check("several fields: each own one that differs, by name", () =>
  assert.equal(
    defaultText(de, ["overlay_style", "overlay_position"], undefined, neutest),
    "Overlay-Position: Unten",
  ),
);
check("an entry: the default's entry", () =>
  assert.equal(
    defaultText(de, ["bindings"], "transcribe", neutest),
    formatKeyCombination("ctrl+option+d", "macos"),
  ),
);
check("a whole list has no default text", () =>
  assert.equal(defaultText(de, ["word_lists"], undefined, neutest), null),
);
check("a coupled field has no default text", () =>
  assert.equal(defaultText(de, ["overlay_style"], undefined, neutest), null),
);

if (failures.length > 0) {
  console.error(`${failures.length} of ${total} checks failed:`);
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(`valueLabels: all ${total} checks passed`);
