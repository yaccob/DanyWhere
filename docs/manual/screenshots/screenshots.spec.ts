// The manual's screenshots, as vector pictures: each shot renders the real
// settings window (backend mocked, `boot.ts`) per language and app theme in
// headless Chromium, prints it as a PDF at the window's size with screen
// styles, and cuts the shot's part out of it as SVG with poppler's
// `pdftocairo`. Text becomes glyph outlines of the open fonts `boot.ts`
// sets, so it stays sharp at any zoom and needs no font on the reader's
// side. svgo and a clip path merge keep the files small.
//
//   bun run manual:screenshots
//
// writes docs/manual/public/screenshots/<lang>/<name>.<theme>.svg. With
// MANUAL_SCREENSHOTS_PREVIEW=<folder> it also writes, per picture, the SVG
// rasterized (`rsvg-convert`), the SVG before svgo rasterized, and plain
// browser screenshots of the same part and of the whole window, to compare.
//
// Needs Playwright's Chromium (`bunx playwright install --only-shell
// chromium`) and poppler (`pdftocairo`); type check with
// `bunx tsc --noEmit -p docs/manual/screenshots/tsconfig.json`.
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { optimize } from "svgo";
import type { Scene } from "./example-data";

const ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const OUT = join(ROOT, "docs/manual/public/screenshots");
const PREVIEW = process.env.MANUAL_SCREENSHOTS_PREVIEW;

const LANGUAGES = ["en", "de", "es", "fr"] as const;
const THEMES = ["light", "dark"] as const;

/** The settings window as DanyWhere opens it (`MAIN_WINDOW_SIZE` in
 * src-tauri/src/utils.rs); its height grows so nothing scrolls. */
const WINDOW = { width: 720, height: 600 };
/** Room around a shot's part, in CSS pixels, so its border doesn't touch
 * the picture's edge. */
const MARGIN = 12;
/** How far a cut-off list shows its next row, fading out, in CSS pixels. */
const FADE = 40;
/** How far the card around a single setting goes on above and below it,
 * fading out, in CSS pixels. */
const SLICE_FADE = 16;
/** CSS pixels to PDF points. */
const PT_PER_PX = 72 / 96;
/** The cut snaps outward to this many points, 4 CSS pixels, so the
 * picture's size is whole pixels. */
const GRID_PT = 3;

/** The only fonts a picture may use (`boot.ts`). */
const FONTS = ["Inter", "JetBrains Mono"];

/** A row of the sidebar: its label's translation key, or a profile's name
 * as it stands. */
type NavStep = { key: string } | { name: string };

interface Shot {
  /** The file name, without language and theme. */
  name: string;
  /** What the window holds (`example-data.ts`); a fresh installation when
   * missing. */
  scene?: Scene;
  /** The sidebar rows clicked one after the other to reach the page, each
   * the first with its label below the one before (a profile's page below
   * the profile); none for Defaults › General, where the window opens
   * (`START_LOCATION`). */
  path?: NavStep[];
  /** The settings group shown, by its title's translation key. */
  groupTitleKey: string;
  /** Show only the group's title, description and first rows of its list;
   * all of it when missing. */
  rows?: number;
  /** Show only this setting of the group, by its title's translation key,
   * with what its scope shows under it (chain, deviating profiles); the
   * whole group when missing. */
  settingTitleKey?: string;
}

const SHOTS: Shot[] = [
  // The first three of the six factory ends: the two with the hold
  // shortcuts' combinations, then the one that starts a paragraph.
  { name: "end-shortcuts", groupTitleKey: "settings.endKeys.title", rows: 3 },
  // The card Instruction Shortcuts with its one factory entry, ⇥ (the
  // chapter "The spoken instruction").
  {
    name: "instruction-shortcuts",
    groupTitleKey: "settings.instructionKeys.title",
  },
  // A profile's page: values that follow the defaults and one of its own
  // (Terminal never post-processes).
  {
    name: "profile-own-values",
    scene: "profiles",
    path: [
      { key: "sidebar.areas.profiles" },
      { name: "Terminal" },
      { key: "sidebar.advanced" },
    ],
    groupTitleKey: "settings.advanced.groups.transcription",
  },
  // A default with the profile that deviates from it, its own control and
  // reset, and the profile that follows.
  {
    name: "defaults-with-profiles",
    scene: "profiles",
    path: [{ key: "sidebar.advanced" }],
    groupTitleKey: "settings.advanced.groups.transcription",
    settingTitleKey: "settings.postProcessing.mode.title",
  },
  // The overview of the profile settings: the profiles with their
  // applications, choosing by application.
  {
    name: "profiles-overview",
    scene: "profiles",
    path: [{ key: "sidebar.areas.profiles" }],
    groupTitleKey: "settings.profiles.title",
  },
];

/** A translation as the app shows it: the locale's text, the product named
 * DanyWhere (`src/i18n/branding.ts`), English where the locale lacks it. */
const translation = (language: string, key: string): string => {
  const read = (code: string) =>
    JSON.parse(
      readFileSync(
        join(ROOT, `src/i18n/locales/${code}/translation.json`),
        "utf8",
      ),
    ) as Record<string, unknown>;
  const lookup = (tree: Record<string, unknown>) =>
    key
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        tree,
      );
  const text = lookup(read(language)) ?? lookup(read("en"));
  if (typeof text !== "string") throw new Error(`no text for ${key}`);
  return text.replace(/\bHandy(?=s?\b)/g, "DanyWhere");
};

/** Opens the window in `language`, `theme` and `scene` and waits until it
 * shows the settings in that language, theme and fonts. */
const openWindow = async (
  page: Page,
  language: string,
  theme: string,
  scene: Scene,
): Promise<void> => {
  await page.setViewportSize(WINDOW);
  await page.emulateMedia({ media: "screen", colorScheme: theme as "light" });
  await page.goto(
    `/docs/manual/screenshots/app.html?lang=${language}&theme=${theme}` +
      (scene === "fresh" ? "" : `&scene=${scene}`),
  );
  await page.waitForFunction(
    ([lang, mode]) =>
      document.documentElement.lang === lang &&
      document.documentElement.dataset.theme === mode &&
      document.querySelector('[id^="end-key-name-"]') !== null,
    [language, theme],
  );
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
};

/** Clicks the sidebar rows of `path` one after the other (`NavStep`) and
 * waits until the page shows the group titled `title` and nothing on it
 * changes any more (the commands it asks are answered). */
const navigate = async (
  page: Page,
  language: string,
  path: NavStep[],
  title: string,
): Promise<void> => {
  for (let step = 1; step <= path.length; step += 1) {
    const labels = path
      .slice(0, step)
      .map((part) =>
        "key" in part ? translation(language, part.key) : part.name,
      );
    // Each row is looked for below the one before, as a person reads the
    // tree: a profile's Advanced, not the defaults' one above it.
    await page.evaluate((wanted) => {
      const rows = [...document.querySelectorAll<HTMLElement>("nav button")];
      let from = 0;
      let row: HTMLElement | undefined;
      for (const label of wanted) {
        const index = rows.findIndex(
          (candidate, at) => at >= from && candidate.title === label,
        );
        if (index < 0) throw new Error(`no sidebar row "${label}"`);
        row = rows[index];
        from = index + 1;
      }
      row?.click();
    }, labels);
  }
  await page.waitForFunction(
    (wanted) =>
      [...document.querySelectorAll("h2")].some(
        (element) => element.textContent?.trim() === wanted,
      ),
    title,
  );
  await page.waitForLoadState("networkidle");
  // Settled: the markup stayed the same over a few frames.
  await page.evaluate(async () => {
    const frame = () =>
      new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      );
    let before = "";
    for (let same = 0; same < 3; ) {
      await frame();
      const now = document.body.innerHTML;
      same = now === before ? same + 1 : 0;
      before = now;
    }
    await document.fonts.ready;
  });
};

/** Lets the window grow until its settings no longer scroll, so the PDF
 * (one page, the window's size) holds all of them. The scrollbars are
 * overlays, as on macOS, so the width of the content stays the same. */
const growWindow = async (page: Page): Promise<void> => {
  for (let round = 0; round < 5; round += 1) {
    const overflow = await page.evaluate(async () => {
      await new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      );
      const scroller = [...document.querySelectorAll<HTMLElement>("div")].find(
        (element) =>
          getComputedStyle(element).overflowY === "auto" &&
          element.scrollHeight > element.clientHeight,
      );
      return scroller ? scroller.scrollHeight - scroller.clientHeight : 0;
    });
    if (overflow === 0) {
      await page.evaluate(() => document.fonts.ready);
      return;
    }
    const size = page.viewportSize() ?? WINDOW;
    await page.setViewportSize({
      width: size.width,
      height: size.height + overflow,
    });
  }
  throw new Error("the settings still scroll");
};

/** Makes the window at least as tall as wide: of a landscape page,
 * pdftocairo turns the cut sideways (`-paperw`/`-paperh` swap). The
 * settings start at the top, so more height only adds room below them. */
const keepUpright = async (page: Page): Promise<void> => {
  const size = page.viewportSize() ?? WINDOW;
  if (size.height >= size.width) return;
  await page.setViewportSize({ width: size.width, height: size.width });
  await page.evaluate(
    () =>
      new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      ),
  );
};

/** The marked part of a shot (`data-manual-shot`). */
const SHOT_SELECTOR = "[data-manual-shot]";

/**
 * Marks the settings group titled `title` as the shot and leaves only what
 * the picture shows visible: the group, of its list only the first `rows`
 * rows and the next one (to fade out), with the window's background. The
 * layout stays as it is, so the PDF holds nothing else. Returns the part's
 * box in CSS pixels, where the list is cut, and the window's background.
 *
 * With `setting`, only the setting with that title shows, with what its
 * scope shows under it, in the card's frame: the group's title and the
 * other settings' contents go invisible, the card's border and the lines
 * between the settings stay. `start` and `cut` are then where the setting
 * begins and ends (the line under it included); the frame fades out above
 * and below.
 *
 * The "?" beside a group's title, which opens the manual's chapter
 * (`ManualHelpButton`), leaves the layout: a picture in the manual needs
 * no way into the manual.
 */
const prepareShot = (
  page: Page,
  title: string,
  rows: number | undefined,
  setting: string | null,
) =>
  page.evaluate(
    ([wanted, shown, only]) => {
      const heading = [...document.querySelectorAll("h2")].find(
        (element) => element.textContent?.trim() === wanted,
      );
      // SettingsGroup: the title's block, then the card, in one element
      // (the only one around the title with `space-y-2`).
      const group = heading?.closest("div.space-y-2");
      if (!group) throw new Error(`no settings group "${wanted}"`);
      group.setAttribute("data-manual-shot", "");
      for (const icon of group.querySelectorAll(
        "svg.lucide-circle-help, svg.lucide-circle-question-mark",
      )) {
        const help = icon.closest("button");
        if (help && heading?.parentElement?.contains(help)) {
          help.style.setProperty("display", "none", "important");
        }
      }
      const hide = (element: Element) => {
        if (element instanceof HTMLElement) {
          element.style.setProperty("visibility", "hidden", "important");
        }
      };
      // Everything beside the group and beside each of its ancestors goes
      // invisible; the group's own elements keep their visibility.
      for (let node: Element = group; node.parentElement; ) {
        const parent: Element = node.parentElement;
        for (const sibling of parent.children) {
          if (sibling !== node) hide(sibling);
        }
        if (parent === document.body) break;
        node = parent;
      }
      const { x, y, width, height } = group.getBoundingClientRect();
      let cut: number | null = null;
      let start: number | null = null;
      if (only !== null) {
        const titled = [...group.querySelectorAll("h3")].find(
          (element) => element.textContent?.trim() === only,
        );
        // The card's list of settings (`SettingsGroup`), and the setting's
        // element in it.
        const list = group.querySelector(":scope > div.border > div");
        let block: Element | null | undefined = titled;
        while (block && block.parentElement !== list) {
          block = block.parentElement;
        }
        if (!block || !list) {
          throw new Error(`no setting "${only}" in "${wanted}"`);
        }
        if (group.firstElementChild !== list.parentElement) {
          hide(group.firstElementChild as Element);
        }
        for (const other of list.children) {
          if (other === block) continue;
          for (const part of other.children) hide(part);
        }
        // The line above it is the setting before's, the line under it its
        // own (Tailwind 4's `divide-y`).
        const { top, bottom } = block.getBoundingClientRect();
        start = top;
        cut = bottom;
      }
      if (shown !== null) {
        // A row of the list: the element around one entry's name field
        // (`<kind>-key-name-<id>`: an end, hold or instruction shortcut)
        // whose parent holds the others too.
        const NAME_FIELD = '[id*="-key-name-"]';
        const names = group.querySelectorAll(NAME_FIELD);
        const rowOf = (field: Element) => {
          let row = field;
          while (
            row.parentElement &&
            row.parentElement.querySelectorAll(NAME_FIELD).length === 1
          ) {
            row = row.parentElement;
          }
          return row;
        };
        const last = names[shown - 1];
        if (!last) throw new Error(`"${wanted}" has fewer than ${shown} rows`);
        const lastRow = rowOf(last);
        cut = lastRow.getBoundingClientRect().bottom;
        // Of what follows, only the next row stays, to fade out.
        let following = lastRow.nextElementSibling?.nextElementSibling;
        while (following) {
          hide(following);
          following = following.nextElementSibling;
        }
      }
      return {
        box: { x, y, width, height },
        start,
        cut,
        background: getComputedStyle(document.documentElement).backgroundColor,
      };
    },
    [title, rows ?? null, setting] as const,
  );

/** The fonts that draw the text of the shot's elements, by family, as
 * Chromium reports them (DevTools' "Rendered Fonts"). */
const fontsOfShot = async (page: Page): Promise<string[]> => {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send("DOM.enable");
    await cdp.send("CSS.enable");
    const { root } = await cdp.send("DOM.getDocument", { depth: -1 });
    const { nodeIds } = await cdp.send("DOM.querySelectorAll", {
      nodeId: root.nodeId,
      selector: `${SHOT_SELECTOR}, ${SHOT_SELECTOR} *`,
    });
    const families = new Set<string>();
    for (const nodeId of nodeIds) {
      const { fonts } = await cdp.send("CSS.getPlatformFontsForNode", {
        nodeId,
      });
      for (const font of fonts) {
        if (font.glyphCount === 0) continue;
        if (font.isCustomFont) {
          // A web font's full name, e.g. "Inter SemiBold": its family.
          families.add(
            FONTS.find((family) => font.familyName.startsWith(family)) ??
              font.familyName,
          );
        } else {
          // A font of this machine's: named with the element it draws.
          const { outerHTML } = await cdp.send("DOM.getOuterHTML", { nodeId });
          families.add(`${font.familyName} in ${outerHTML.slice(0, 160)}`);
        }
      }
    }
    return [...families].sort();
  } finally {
    await cdp.detach();
  }
};

/** Merges cairo's identical clip paths into one each (it writes one per
 * use), pointing every reference at the one kept. */
const mergeClipPaths = (svg: string): string => {
  let merged = svg;
  for (let before = ""; before !== merged; ) {
    before = merged;
    const kept = new Map<string, string>();
    const alias = new Map<string, string>();
    merged = merged.replace(
      /<clipPath id="([^"]+)">([\s\S]*?)<\/clipPath>\n?/g,
      (whole, id: string, body: string) => {
        const first = kept.get(body);
        if (first === undefined) {
          kept.set(body, id);
          return whole;
        }
        alias.set(id, first);
        return "";
      },
    );
    merged = merged.replace(
      /url\(#([^)]+)\)/g,
      (whole, id: string) => `url(#${alias.get(id) ?? id})`,
    );
  }
  return merged;
};

/** pdftocairo's SVG sized in whole CSS pixels; the PDF's points stay its
 * coordinates (`viewBox`). */
const sizeSvg = (svg: string, widthPt: number, heightPt: number): string =>
  svg.replace(
    /<svg ([^>]*?)width="[^"]+" height="[^"]+"/,
    `<svg $1width="${widthPt / PT_PER_PX}" height="${heightPt / PT_PER_PX}"`,
  );

/** The SVG for the site: sized in CSS pixels, where the list was cut its
 * last part fading into the window's background (and the first part, where
 * the picture starts inside a card: `fadeIn`), clip paths merged, and
 * svgo's safe default optimizations. */
const finishSvg = (
  raw: string,
  widthPt: number,
  heightPt: number,
  fade: { heightPt: number; color: string } | null,
  fadeIn: { heightPt: number; color: string } | null = null,
): string => {
  const sized = sizeSvg(raw, widthPt, heightPt);
  const fadedOut = fade
    ? sized.replace(
        /<\/svg>\s*$/,
        `<linearGradient id="fade" x1="0" y1="0" x2="0" y2="1">` +
          `<stop offset="0" stop-color="${fade.color}" stop-opacity="0"/>` +
          `<stop offset="1" stop-color="${fade.color}"/>` +
          `</linearGradient>` +
          `<rect x="0" y="${heightPt - fade.heightPt}" width="${widthPt}" ` +
          `height="${fade.heightPt}" fill="url(#fade)"/></svg>\n`,
      )
    : sized;
  const faded = fadeIn
    ? fadedOut.replace(
        /<\/svg>\s*$/,
        `<linearGradient id="fade-in" x1="0" y1="0" x2="0" y2="1">` +
          `<stop offset="0" stop-color="${fadeIn.color}"/>` +
          `<stop offset="1" stop-color="${fadeIn.color}" stop-opacity="0"/>` +
          `</linearGradient>` +
          `<rect x="0" y="0" width="${widthPt}" ` +
          `height="${fadeIn.heightPt}" fill="url(#fade-in)"/></svg>\n`,
      )
    : fadedOut;
  return optimize(mergeClipPaths(faded), {
    multipass: true,
    floatPrecision: 2,
    plugins: ["preset-default"],
  }).data;
};

for (const language of LANGUAGES) {
  for (const theme of THEMES) {
    test(`${language} ${theme}`, async ({ page }) => {
      // An error in the app would leave a picture of a half-drawn window.
      const errors: string[] = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      for (const shot of SHOTS) {
        const title = translation(language, shot.groupTitleKey);
        await openWindow(page, language, theme, shot.scene ?? "fresh");
        if (shot.path) await navigate(page, language, shot.path, title);
        await growWindow(page);
        await keepUpright(page);
        const viewport = page.viewportSize();
        if (!viewport) throw new Error("no viewport");
        const preview =
          PREVIEW && join(PREVIEW, `${shot.name}.${language}.${theme}`);
        if (preview) {
          // The window as the browser shows it, before anything is hidden.
          mkdirSync(PREVIEW as string, { recursive: true });
          await page.screenshot({ path: `${preview}.window.png` });
        }
        const setting = shot.settingTitleKey
          ? translation(language, shot.settingTitleKey)
          : null;
        const { box, start, cut, background } = await prepareShot(
          page,
          title,
          shot.rows,
          setting,
        );
        // Open fonts only, on any machine (`boot.ts`).
        const fonts = await fontsOfShot(page);
        expect(fonts).toContain("Inter");
        expect(fonts.filter((family) => !FONTS.includes(family))).toEqual([]);
        // The part with its margin, or down to the cut and its fade, in
        // points (pdftocairo's unit), snapped outward to the grid.
        const snap = (px: number, round: (n: number) => number) =>
          round((px * PT_PER_PX) / GRID_PT) * GRID_PT;
        const fadeLength = setting === null ? FADE : SLICE_FADE;
        const left = snap(box.x - MARGIN, Math.floor);
        const top =
          start === null
            ? snap(box.y - MARGIN, Math.floor)
            : snap(start - fadeLength, Math.floor);
        const right = snap(box.x + box.width + MARGIN, Math.ceil);
        const bottom =
          cut === null
            ? snap(box.y + box.height + MARGIN, Math.ceil)
            : snap(cut + fadeLength, Math.ceil);
        const [width, height] = [right - left, bottom - top];

        const work = mkdtempSync(join(tmpdir(), "manual-screenshots-"));
        try {
          const pdf = join(work, "window.pdf");
          const cutSvg = join(work, "cut.svg");
          await page.pdf({
            path: pdf,
            width: `${viewport.width}px`,
            height: `${viewport.height}px`,
            margin: { top: "0", right: "0", bottom: "0", left: "0" },
            printBackground: true,
            scale: 1,
            pageRanges: "1",
          });
          // `-nocenter` puts the cut at the top left of a page its size.
          execFileSync("pdftocairo", [
            "-svg",
            ...["-f", "1", "-l", "1"],
            ...["-x", String(left), "-y", String(top)],
            ...["-W", String(width), "-H", String(height)],
            ...["-paperw", String(width), "-paperh", String(height)],
            "-nocenter",
            pdf,
            cutSvg,
          ]);
          const raw = readFileSync(cutSvg, "utf8");
          // Vector only: a bitmap (a colour emoji, an image in the UI)
          // would blur when zoomed.
          expect(raw).not.toContain("<image");
          // From the cut, in the picture's points, to its bottom.
          const fadeHeight =
            cut === null ? null : height - (cut * PT_PER_PX - top);
          // From the picture's top to where the setting starts.
          const fadeInHeight = start === null ? null : start * PT_PER_PX - top;
          const svg = finishSvg(
            raw,
            width,
            height,
            fadeHeight === null
              ? null
              : { heightPt: fadeHeight, color: background },
            fadeInHeight === null
              ? null
              : { heightPt: fadeInHeight, color: background },
          );
          expect(svg).toContain("<svg");
          const dir = join(OUT, language);
          mkdirSync(dir, { recursive: true });
          const file = join(dir, `${shot.name}.${theme}.svg`);
          writeFileSync(file, svg);

          if (preview) {
            // The same part as the browser shows it, the SVG before and
            // after svgo drawn by another renderer, all at twice the size.
            await page.screenshot({
              path: `${preview}.screen.png`,
              clip: {
                x: left / PT_PER_PX,
                y: top / PT_PER_PX,
                width: width / PT_PER_PX,
                height: height / PT_PER_PX,
              },
            });
            const before = join(work, "before.svg");
            writeFileSync(before, sizeSvg(raw, width, height));
            execFileSync("rsvg-convert", [
              ...["-z", "2", "-o", `${preview}.raw.png`],
              before,
            ]);
            execFileSync("rsvg-convert", [
              ...["-z", "2", "-o", `${preview}.svg.png`],
              file,
            ]);
          }
        } finally {
          rmSync(work, { recursive: true, force: true });
        }
      }
      expect(errors).toEqual([]);
    });
  }
}
