// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/styles/searchMatchContrast.test.ts
//
// The History search marks its matches with two theme tokens:
// --color-search-match (the fill) and --color-search-match-text (the text on
// it). A match has to be seen at a glance in either theme, so per theme:
// - the fill against the surface the History text sits on reaches 3:1
//   (WCAG 1.4.11, non-text contrast);
// - the text on the fill reaches 4.5:1 (WCAG 1.4.3) in light, 3:1 in dark
//   (see MIN_TEXT).
// The fills are petrol, as the user picked them, so the version track marks
// its shown chip with a ring of --color-shown-version outside the chip, with
// a gap: a petrol edge on the petrol fill would hardly show. The ring reaches
// 3:1 against the surface it is drawn on.
// Colors are read from theme.css and alpha-composited the way the browser
// draws them, so a tint like "25 % petrol" is measured as it looks.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (path: string) =>
  readFileSync(new URL(path, import.meta.url), "utf8");

let checks = 0;
let failures = 0;
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

// --- Reading the CSS --------------------------------------------------------

const stripComments = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, "");

/** The body of the first block opened by `prelude {`. */
const blockBody = (css: string, prelude: string): string => {
  const start = css.indexOf(`${prelude} {`);
  if (start === -1) throw new Error(`no block "${prelude}"`);
  const open = css.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < css.length; i += 1) {
    if (css[i] === "{") depth += 1;
    else if (css[i] === "}" && (depth -= 1) === 0) {
      return css.slice(open + 1, i);
    }
  }
  throw new Error(`block "${prelude}" is not closed`);
};

/** The custom properties a block declares, values on one line. */
const declarations = (body: string): Map<string, string> => {
  const found = new Map<string, string>();
  for (const part of body.split(";")) {
    const match = part.match(/^\s*(--[\w-]+)\s*:\s*([\s\S]+?)\s*$/);
    if (match) found.set(match[1], match[2].replace(/\s+/g, " "));
  }
  return found;
};

// --- Colors -----------------------------------------------------------------

/** sRGB channels and alpha, each 0..1, not premultiplied. */
type Rgba = { r: number; g: number; b: number; a: number };

/** `list` split at its top-level commas. */
const splitArguments = (list: string): string[] => {
  const parts: string[] = [];
  let depth = 0;
  let from = 0;
  for (let i = 0; i < list.length; i += 1) {
    if (list[i] === "(") depth += 1;
    else if (list[i] === ")") depth -= 1;
    else if (list[i] === "," && depth === 0) {
      parts.push(list.slice(from, i).trim());
      from = i + 1;
    }
  }
  parts.push(list.slice(from).trim());
  return parts;
};

const fromHex = (hex: string): Rgba => {
  const digits =
    hex.length === 3
      ? hex
          .split("")
          .map((digit) => digit + digit)
          .join("")
      : hex;
  const channel = (at: number) => parseInt(digits.slice(at, at + 2), 16) / 255;
  return { r: channel(0), g: channel(2), b: channel(4), a: 1 };
};

/** `color-mix(in srgb, A p%, B q%)` as CSS Color 5 mixes it: premultiplied
 * alpha, missing percentages filled in, a sum under 100 % lowering alpha. */
const mix = (first: Rgba, p: number, second: Rgba, q: number): Rgba => {
  const total = p + q;
  const w1 = p / total;
  const w2 = q / total;
  const a = first.a * w1 + second.a * w2;
  const scale = total < 100 ? total / 100 : 1;
  if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
  const channel = (key: "r" | "g" | "b") =>
    (first[key] * first.a * w1 + second[key] * second.a * w2) / a;
  return {
    r: channel("r"),
    g: channel("g"),
    b: channel("b"),
    a: a * scale,
  };
};

/** A color value of theme.css with its `var()`s looked up in `vars`. */
const resolve = (value: string, vars: Map<string, string>): Rgba => {
  const text = value.trim();
  const reference = text.match(/^var\(\s*(--[\w-]+)\s*\)$/);
  if (reference) {
    const next = vars.get(reference[1]);
    if (next === undefined) throw new Error(`${reference[1]} is not defined`);
    return resolve(next, vars);
  }
  if (text === "transparent") return { r: 0, g: 0, b: 0, a: 0 };
  const hex = text.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) return fromHex(hex[1]);
  const mixed = text.match(/^color-mix\(([\s\S]*)\)$/);
  if (mixed) {
    const [space, ...colors] = splitArguments(mixed[1]);
    if (space !== "in srgb" || colors.length !== 2) {
      throw new Error(`can't measure ${text}`);
    }
    const [first, second] = colors.map((part) => {
      const weighted = part.match(/^([\s\S]+?)\s+(\d+(?:\.\d+)?)%$/);
      return weighted
        ? { color: resolve(weighted[1], vars), percent: Number(weighted[2]) }
        : { color: resolve(part, vars), percent: undefined };
    });
    const p = first.percent ?? 100 - (second.percent ?? 50);
    const q = second.percent ?? 100 - p;
    return mix(first.color, p, second.color, q);
  }
  throw new Error(`can't measure ${text}`);
};

/** `top` drawn over the opaque `bottom`. */
const over = (top: Rgba, bottom: Rgba): Rgba => ({
  r: top.r * top.a + bottom.r * (1 - top.a),
  g: top.g * top.a + bottom.g * (1 - top.a),
  b: top.b * top.a + bottom.b * (1 - top.a),
  a: 1,
});

const luminance = ({ r, g, b }: Rgba) => {
  const linear = (c: number) =>
    c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
};

/** WCAG contrast ratio of two opaque colors. */
const contrast = (one: Rgba, other: Rgba) => {
  const [light, dark] = [luminance(one), luminance(other)].sort(
    (x, y) => y - x,
  );
  return (light + 0.05) / (dark + 0.05);
};

const toHex = ({ r, g, b }: Rgba) =>
  `#${[r, g, b]
    .map((c) =>
      Math.round(c * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;

// --- The themes -------------------------------------------------------------

const theme = stripComments(read("./theme.css"));
const base = declarations(blockBody(theme, ":root"));
const osDark = declarations(
  blockBody(blockBody(theme, "@media (prefers-color-scheme: dark)"), ":root"),
);
const setLight = declarations(blockBody(theme, ':root[data-theme="light"]'));
const setDark = declarations(blockBody(theme, ':root[data-theme="dark"]'));

const THEMES = {
  light: base,
  dark: new Map([...base, ...osDark]),
};
const TOKENS = [
  "--color-search-match",
  "--color-search-match-text",
  "--color-shown-version",
];

// The user's choice on 2026-11-15 from the sheet
// qa/out/2026-11-15_2250_fundstellen-petrol/abbildungen/varianten.svg in the
// sidecar ("D1 und L2"):
// - dark, D1: the selected sidebar item's petrol (logo-primary at 80 % over
//   the background) with white text;
// - light, L2: logo-primary, only as much darker as 3:1 needs, with dark text.
const CHOSEN = {
  light: { fill: "#3999a7", text: "#0f0f0f" },
  dark: { fill: "#5298a2", text: "#fbfbfb" },
};

const MIN_FILL = 3;
const MIN_RING = 3;
// D1's white text reads at 3.19:1 on its fill, under WCAG's 4.5:1. The user
// saw that failure on the sheet and chose D1 anyway on 2026-11-15, to match
// the selected sidebar item; dark mode therefore only guards 3:1.
const MIN_TEXT = { light: 4.5, dark: 3 };

for (const [name, vars] of Object.entries(THEMES) as [
  keyof typeof THEMES,
  Map<string, string>,
][]) {
  const minText = MIN_TEXT[name];
  // The History list is a `bg-background` card.
  const surface = resolve("var(--color-background)", vars);
  let fill: Rgba | null = null;
  let text: Rgba | null = null;
  try {
    fill = over(resolve("var(--color-search-match)", vars), surface);
    text = over(resolve("var(--color-search-match-text)", vars), fill);
    console.log(
      `${name}: fill ${toHex(fill)} on ${toHex(surface)} ` +
        `${contrast(fill, surface).toFixed(2)}:1 (needs ${MIN_FILL}), ` +
        `text ${toHex(text)} on the fill ` +
        `${contrast(text, fill).toFixed(2)}:1 (needs ${minText})`,
    );
  } catch (error) {
    console.error(`${name}: ${(error as Error).message}`);
  }

  check(`${name}: a match's fill stands out from the History surface`, () => {
    assert.ok(fill, "no match fill");
    const ratio = contrast(fill, surface);
    assert.ok(
      ratio >= MIN_FILL,
      `fill ${toHex(fill)} on ${toHex(surface)}: ${ratio.toFixed(2)}:1, ` +
        `needs ${MIN_FILL}:1`,
    );
  });

  check(`${name}: a match's text reads on its fill`, () => {
    assert.ok(fill && text, "no match colors");
    const ratio = contrast(text, fill);
    assert.ok(
      ratio >= minText,
      `text ${toHex(text)} on ${toHex(fill)}: ${ratio.toFixed(2)}:1, ` +
        `needs ${minText}:1`,
    );
  });

  check(`${name}: the match colors are the ones the user chose`, () => {
    assert.ok(fill && text, "no match colors");
    assert.equal(toHex(fill), CHOSEN[name].fill, "fill");
    assert.equal(toHex(text), CHOSEN[name].text, "text");
  });

  check(`${name}: the shown version's ring stands out from the surface`, () => {
    const ring = over(resolve("var(--color-shown-version)", vars), surface);
    const ratio = contrast(ring, surface);
    console.log(
      `${name}: ring ${toHex(ring)} on ${toHex(surface)} ` +
        `${ratio.toFixed(2)}:1 (needs ${MIN_RING})`,
    );
    assert.ok(
      ratio >= MIN_RING,
      `ring ${toHex(ring)} on ${toHex(surface)}: ${ratio.toFixed(2)}:1, ` +
        `needs ${MIN_RING}:1`,
    );
  });
}

// theme.css sets the active tokens in three places; a token missed in one of
// them would show the other theme's match colors.
check("the Dark setting shows the same matches as dark macOS", () => {
  const setting = new Map([...base, ...setDark]);
  for (const token of TOKENS) {
    assert.equal(
      toHex(resolve(`var(${token})`, setting)),
      toHex(resolve(`var(${token})`, THEMES.dark)),
      token,
    );
  }
});

check("the Light setting shows the same matches as light macOS", () => {
  const setting = new Map([...base, ...setLight]);
  for (const token of TOKENS) {
    assert.equal(
      toHex(resolve(`var(${token})`, setting)),
      toHex(resolve(`var(${token})`, THEMES.light)),
      token,
    );
  }
});

// Tailwind drops a class whose color it doesn't know without a word.
check("Tailwind knows the match colors", () => {
  const registered = declarations(
    blockBody(stripComments(read("../App.css")), "@theme inline"),
  );
  for (const token of TOKENS) {
    assert.equal(registered.get(token), `var(${token})`, token);
  }
});

check("the History search draws every match with the match colors", () => {
  const history = "../components/settings/history/";
  for (const file of ["HistorySearchBar.tsx", "VersionTrack.tsx"]) {
    const source = read(`${history}${file}`);
    assert.ok(source.includes("bg-search-match"), `${file}: no match fill`);
    assert.ok(
      source.includes("text-search-match-text"),
      `${file}: no match text color`,
    );
    assert.ok(
      !source.includes("bg-logo-primary/25"),
      `${file}: still the petrol tint`,
    );
  }
});

// The ring sits outside the chip with a gap of the surface's color, so it
// never touches the match fill.
check("the version track rings the shown chip", () => {
  const source = read("../components/settings/history/VersionTrack.tsx");
  for (const name of [
    "ring-1",
    "ring-shown-version",
    "ring-offset-1",
    "ring-offset-background",
  ]) {
    assert.ok(source.includes(name), `VersionTrack.tsx: no ${name}`);
  }
});

if (failures > 0) {
  console.error(
    `searchMatchContrast: ${failures} of ${checks} checks failed, ${checks - failures} passed`,
  );
  process.exit(1);
}
console.log(`searchMatchContrast: all ${checks} checks passed`);
