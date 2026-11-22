/**
 * Keyboard utility functions for handling keyboard events
 */

import type { KeyboardEvent as ReactKeyboardEvent } from "react";

export type OSType = "macos" | "windows" | "linux" | "unknown";

/** IME boundary keydowns can have isComposing=false while keyCode is still 229. */
export const isComposingKeyEvent = (
  event: Pick<KeyboardEvent, "isComposing" | "keyCode">,
): boolean => event.isComposing || event.keyCode === 229;

const keepSpace = (e: ReactKeyboardEvent<HTMLElement>) => {
  if (e.key === " ") e.preventDefault();
};

/** Spread on shortcut controls so a Space in the shortcut doesn't click them. */
export const spaceDoesNotClick = { onKeyDown: keepSpace, onKeyUp: keepSpace };

// Shortcut parsers require compact tokens; keep display labels separate.
const COMPOUND_KEY_DISPLAY: Record<string, string> = {
  capslock: "Caps Lock",
  numlock: "Num Lock",
  pageup: "Page Up",
  pagedown: "Page Down",
  printscreen: "Print Screen",
  scrolllock: "Scroll Lock",
};

/**
 * Extract a consistent key name from a KeyboardEvent
 * This function provides cross-platform keyboard event handling
 * and returns key names appropriate for the target operating system
 */
export const getKeyName = (
  e: KeyboardEvent,
  osType: OSType = "unknown",
): string => {
  // Handle special cases first
  if (e.code) {
    const code = e.code;

    // Handle function keys (F1-F24)
    if (code.match(/^F\d+$/)) {
      return code.toLowerCase(); // F1, F2, ..., F14, F15, etc.
    }

    // Handle regular letter keys (KeyA -> a)
    if (code.match(/^Key[A-Z]$/)) {
      return code.replace("Key", "").toLowerCase();
    }

    // Handle digit keys (Digit0 -> 0)
    if (code.match(/^Digit\d$/)) {
      return code.replace("Digit", "");
    }

    // Handle numpad digit keys (Numpad0 -> numpad 0)
    if (code.match(/^Numpad\d$/)) {
      return code.replace("Numpad", "numpad ").toLowerCase();
    }

    // Handle modifier keys - OS-specific naming
    const getModifierName = (baseModifier: string): string => {
      switch (baseModifier) {
        case "shift":
          return "shift";
        case "ctrl":
          return osType === "macos" ? "ctrl" : "ctrl";
        case "alt":
          return osType === "macos" ? "option" : "alt";
        case "meta":
          // Windows key on Windows/Linux, Command key on Mac
          if (osType === "macos") return "command";
          return "super";
        default:
          return baseModifier;
      }
    };

    const modifierMap: Record<string, string> = {
      ShiftLeft: getModifierName("shift"),
      ShiftRight: getModifierName("shift"),
      ControlLeft: getModifierName("ctrl"),
      ControlRight: getModifierName("ctrl"),
      AltLeft: getModifierName("alt"),
      AltRight: getModifierName("alt"),
      MetaLeft: getModifierName("meta"),
      MetaRight: getModifierName("meta"),
      OSLeft: getModifierName("meta"),
      OSRight: getModifierName("meta"),
      CapsLock: "capslock",
      Tab: "tab",
      Enter: "enter",
      Space: "space",
      Backspace: "backspace",
      Delete: "delete",
      Escape: "esc",
      ArrowUp: "up",
      ArrowDown: "down",
      ArrowLeft: "left",
      ArrowRight: "right",
      Home: "home",
      End: "end",
      PageUp: "pageup",
      PageDown: "pagedown",
      Insert: "insert",
      PrintScreen: "printscreen",
      ScrollLock: "scrolllock",
      Pause: "pause",
      ContextMenu: "menu",
      NumpadMultiply: "numpad *",
      NumpadAdd: "numpad +",
      NumpadSubtract: "numpad -",
      NumpadDecimal: "numpad .",
      NumpadDivide: "numpad /",
      NumLock: "numlock",
    };

    if (modifierMap[code]) {
      return modifierMap[code];
    }

    // Handle punctuation and special characters
    const punctuationMap: Record<string, string> = {
      Semicolon: ";",
      Equal: "=",
      Comma: ",",
      Minus: "-",
      Period: ".",
      Slash: "/",
      Backquote: "`",
      BracketLeft: "[",
      Backslash: "\\",
      BracketRight: "]",
      Quote: "'",
    };

    if (punctuationMap[code]) {
      return punctuationMap[code];
    }

    // Parser names mirror KeyboardEvent.code without spaces.
    return code.toLowerCase();
  }

  // Fallback to e.key if e.code is not available
  if (e.key) {
    const key = e.key;

    // Handle special key names with OS-specific formatting
    const keyMap: Record<string, string> = {
      Control: osType === "macos" ? "ctrl" : "ctrl",
      Alt: osType === "macos" ? "option" : "alt",
      Shift: "shift",
      Meta:
        osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
      OS:
        osType === "macos" ? "command" : osType === "windows" ? "win" : "super",
      CapsLock: "capslock",
      ArrowUp: "up",
      ArrowDown: "down",
      ArrowLeft: "left",
      ArrowRight: "right",
      Escape: "esc",
      " ": "space",
    };

    if (keyMap[key]) {
      return keyMap[key];
    }

    return key.toLowerCase();
  }

  // Last resort fallback
  return `unknown-${e.keyCode || e.which || 0}`;
};

/**
 * Capitalize a key name for display (e.g. "space" -> "Space", "f1" -> "F1")
 */
const capitalizeKey = (key: string): string => {
  // fn key: keep lowercase
  if (key === "fn") return "fn";
  // Function keys: f1 -> F1
  if (/^f\d+$/.test(key)) return key.toUpperCase();
  // Single char: a -> A
  if (key.length === 1) return key.toUpperCase();
  if (COMPOUND_KEY_DISPLAY[key]) return COMPOUND_KEY_DISPLAY[key];
  // Multi-word: capitalize first letter of each word
  return key.replace(/\b\w/g, (c) => c.toUpperCase());
};

/** Translates a `keyNames.*` text, as i18next's `t` does. */
export type KeyNameT = (
  key: string,
  options?: Record<string, unknown>,
) => string;

/**
 * DanyWhere: the English texts of `keyNames.*` (the locale's), for callers
 * without `t`; keyNames.test.ts checks they match.
 */
export const ENGLISH_KEY_NAMES: Record<string, string> = {
  ctrl: "Ctrl",
  shift: "Shift",
  space: "Space",
  enter: "Enter",
  backspace: "Backspace",
  delete: "Delete",
  insert: "Insert",
  home: "Home",
  end: "End",
  pageUp: "Page Up",
  pageDown: "Page Down",
  capsLock: "Caps Lock",
  numLock: "Num Lock",
  scrollLock: "Scroll Lock",
  printScreen: "Print Screen",
  pause: "Pause",
  menu: "Menu",
  keypad: "Keypad {{key}}",
  playPause: "Play/Pause",
  stop: "Stop",
  previousTrack: "Previous Track",
  nextTrack: "Next Track",
  mouseLeft: "Left Mouse Button",
  mouseRight: "Right Mouse Button",
  mouseMiddle: "Middle Mouse Button",
  mouseBack: "Mouse Button 4",
  mouseForward: "Mouse Button 5",
  dictation: "Dictation",
  left: "Left {{key}}",
  right: "Right {{key}}",
};

const englishT: KeyNameT = (key, options = {}) =>
  (ENGLISH_KEY_NAMES[key.replace(/^keyNames\./, "")] ?? key).replace(
    /\{\{(\w+)\}\}/g,
    (_, name: string) => String(options[name] ?? ""),
  );

type ModifierKind = "fn" | "ctrl" | "alt" | "shift" | "super";

const MODIFIER_KINDS: Record<string, ModifierKind> = {
  fn: "fn",
  function: "fn",
  ctrl: "ctrl",
  control: "ctrl",
  option: "alt",
  opt: "alt",
  alt: "alt",
  shift: "shift",
  command: "super",
  cmd: "super",
  meta: "super",
  super: "super",
  win: "super",
  windows: "super",
};

// The order of the system's menus: ⌃⌥⇧⌘ on macOS, Ctrl+Alt+Shift+Win elsewhere.
const MAC_ORDER: ModifierKind[] = ["fn", "ctrl", "alt", "shift", "super"];
const PC_ORDER: ModifierKind[] = ["ctrl", "alt", "shift", "super", "fn"];

const MAC_MODIFIERS: Record<ModifierKind, string> = {
  fn: "fn",
  ctrl: "⌃",
  alt: "⌥",
  shift: "⇧",
  super: "⌘",
};

const ARROWS: Record<string, string> = {
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
};

// Symbols on Mac keyboards and in macOS menus.
const MAC_KEYS: Record<string, string> = {
  ...ARROWS,
  return: "↩",
  enter: "↩",
  tab: "⇥",
  // handy-keys, macOS's backend, calls Backspace "delete".
  delete: "⌫",
  backspace: "⌫",
  forwarddelete: "⌦",
  escape: "esc",
  esc: "esc",
  home: "↖",
  end: "↘",
  pageup: "⇞",
  pagedown: "⇟",
  capslock: "⇪",
  keypadenter: "⌤",
};

// Texts (`keyNames.*`) for keys whose name is a word.
const NAMED_KEYS: Record<string, string> = {
  space: "space",
  return: "enter",
  enter: "enter",
  backspace: "backspace",
  forwarddelete: "delete",
  insert: "insert",
  home: "home",
  end: "end",
  pageup: "pageUp",
  pagedown: "pageDown",
  capslock: "capsLock",
  numlock: "numLock",
  scrolllock: "scrollLock",
  printscreen: "printScreen",
  pause: "pause",
  contextmenu: "menu",
  menu: "menu",
  playpause: "playPause",
  stop: "stop",
  prevtrack: "previousTrack",
  nexttrack: "nextTrack",
  mouseleft: "mouseLeft",
  mouseright: "mouseRight",
  mousemiddle: "mouseMiddle",
  mousex1: "mouseBack",
  mousex2: "mouseForward",
  dictation: "dictation",
};

const KEYPAD_SIGNS: Record<string, string> = {
  plus: "+",
  minus: "-",
  multiply: "*",
  divide: "/",
  decimal: ".",
  equals: "=",
  comma: ",",
  clear: "⌧",
};

const CONSTANT_KEYS: Record<string, string> = {
  tab: "Tab",
  escape: "Esc",
  esc: "Esc",
  jiseisu: "英数",
  eisu: "英数",
  jiskana: "かな",
  kana: "かな",
  jisunderscore: "_",
};

/** A keypad key's own part ("1", "+"), or null for another key. */
const keypadPart = (name: string): string | null => {
  const match = /^(?:keypad|numpad )(.+)$/.exec(name);
  if (!match) return null;
  return KEYPAD_SIGNS[match[1]] ?? match[1].toUpperCase();
};

/** A main key's label on `osType`. */
const keyLabel = (name: string, osType: OSType, t: KeyNameT): string => {
  if (osType === "macos" && MAC_KEYS[name]) return MAC_KEYS[name];
  if (ARROWS[name]) return ARROWS[name];
  // On Windows handy-keys calls Backspace "delete"; on Linux the Tauri
  // shortcuts call the Delete key so.
  if (name === "delete")
    return t(osType === "windows" ? "keyNames.backspace" : "keyNames.delete");
  if (name === "keypadenter")
    return t("keyNames.keypad", { key: t("keyNames.enter") });
  const keypad = keypadPart(name);
  if (keypad !== null) return t("keyNames.keypad", { key: keypad });
  if (NAMED_KEYS[name]) return t(`keyNames.${NAMED_KEYS[name]}`);
  if (CONSTANT_KEYS[name]) return CONSTANT_KEYS[name];
  return capitalizeKey(name);
};

/** A modifier's label on `osType`. */
const modifierLabel = (
  kind: ModifierKind,
  osType: OSType,
  t: KeyNameT,
): string => {
  if (osType === "macos") return MAC_MODIFIERS[kind];
  switch (kind) {
    case "ctrl":
      return t("keyNames.ctrl");
    case "shift":
      return t("keyNames.shift");
    case "alt":
      return "Alt";
    case "super":
      return osType === "windows" ? "Win" : "Super";
    case "fn":
      return "Fn";
  }
};

/** A label written in letters, which macOS sets apart by a space. */
const isWord = (label: string): boolean =>
  /\p{L}{2,}/u.test(label) || label.includes(" ");

/**
 * DanyWhere: a stored combination as the system names it (user,
 * 2026-11-22). macOS writes the symbols on the keys together, in the
 * order of its menus ("⌃⌥D", "⌃↩", "⌥ Leertaste"); Windows and Linux write
 * the names on the keys, translated, joined with "+" ("Strg+Umschalt+V").
 * A side shows only where the combination keeps one (`key_sides.rs`):
 * "⌥ rechts", "Alt links+Q". Without `t` the names are English.
 */
export const formatKeyCombination = (
  combination: string,
  osType: OSType,
  t: KeyNameT = englishT,
): string => {
  if (!combination) return "";
  const modifiers: { kind: ModifierKind; side: string | null }[] = [];
  const keys: string[] = [];
  for (const raw of combination.split("+")) {
    const part = raw.trim().toLowerCase();
    if (!part) continue;
    const sided = /^(.+)_(left|right)$/.exec(part);
    const name = sided && MODIFIER_KINDS[sided[1]] ? sided[1] : part;
    const kind = MODIFIER_KINDS[name];
    if (kind) modifiers.push({ kind, side: sided ? sided[2] : null });
    else keys.push(keyLabel(part, osType, t));
  }
  const order = osType === "macos" ? MAC_ORDER : PC_ORDER;
  modifiers.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind));
  const labels = [
    ...modifiers.map(({ kind, side }) => {
      const label = modifierLabel(kind, osType, t);
      return side ? t(`keyNames.${side}`, { key: label }) : label;
    }),
    ...keys,
  ];
  if (osType !== "macos") return labels.join("+");
  return labels
    .map((label, i) =>
      i > 0 && (isWord(label) || isWord(labels[i - 1])) ? ` ${label}` : label,
    )
    .join("");
};

/** The keys "Clipboard (…)" pastes with. */
export const clipboardPasteKeys = (osType: OSType): string =>
  osType === "macos" ? "command+v" : "ctrl+v";

/**
 * Normalize modifier keys to handle left/right variants
 */
export const normalizeKey = (key: string): string => {
  // Handle left/right variants of modifier keys
  if (key.startsWith("left ") || key.startsWith("right ")) {
    const parts = key.split(" ");
    if (parts.length === 2) {
      // Return just the modifier name without left/right prefix
      return parts[1];
    }
  }
  return key;
};

/** Keys that only modify another: a binding needs a key besides these. */
export const MODIFIERS = [
  "ctrl",
  "control",
  "shift",
  "alt",
  "option",
  "meta",
  "command",
  "cmd",
  "super",
  "win",
  "windows",
  "fn",
];

/** The keys in a binding, or in one still being captured ("option+space"). */
export const bindingKeys = (binding: string | null | undefined): string[] =>
  binding ? binding.split("+").filter(Boolean) : [];

/** Only modifiers are held so far: the binding still needs a key. */
export const isOnlyModifiers = (keys: string[]): boolean =>
  keys.length > 0 && keys.every((k) => MODIFIERS.includes(k.toLowerCase()));

// Labels as printed on the keys; a key's side (option_left) isn't printed.
const MAC_KEYCAPS: Record<string, string> = {
  option: "⌥",
  alt: "⌥",
  command: "⌘",
  cmd: "⌘",
  meta: "⌘",
  super: "⌘",
  ctrl: "⌃",
  control: "⌃",
  shift: "⇧",
};
const PC_KEYCAPS: Record<string, string> = {
  alt: "Alt",
  option: "Alt",
  ctrl: "Ctrl",
  control: "Ctrl",
  shift: "Shift",
  meta: "⊞",
  super: "⊞",
  command: "⊞",
};

/** "option+space" → ["⌥", "Space"] on macOS. */
export const keycapLabels = (binding: string, osType: OSType): string[] =>
  binding
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const key = part.replace(/_(left|right)$/, "");
      const glyphs = osType === "macos" ? MAC_KEYCAPS : PC_KEYCAPS;
      return glyphs[key] ?? capitalizeKey(key);
    });

/** "⌥ Space" on macOS, "Alt + Space" elsewhere. */
export const keycapLabel = (binding: string, osType: OSType): string =>
  keycapLabels(binding, osType).join(osType === "macos" ? " " : " + ");
