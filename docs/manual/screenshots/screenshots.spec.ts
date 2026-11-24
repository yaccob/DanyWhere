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
/** CSS pixels to PDF points. */
const PT_PER_PX = 72 / 96;
/** The cut snaps outward to this many points, 4 CSS pixels, so the
 * picture's size is whole pixels. */
const GRID_PT = 3;

/** The only fonts a picture may use (`boot.ts`). */
const FONTS = ["Inter", "JetBrains Mono"];

interface Shot {
  /** The file name, without language and theme. */
  name: string;
  /** The settings group shown, by its title's translation key. */
  groupTitleKey: string;
  /** Show only the group's title, description and first rows of its list;
   * all of it when missing. */
  rows?: number;
}

/** Defaults › General is where the window opens (`START_LOCATION`). */
const SHOTS: Shot[] = [
  { name: "end-shortcuts", groupTitleKey: "settings.endKeys.title", rows: 4 },
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

/** Opens the window in `language` and `theme` and waits until it shows the
 * settings in that language, theme and fonts. */
const openWindow = async (
  page: Page,
  language: string,
  theme: string,
): Promise<void> => {
  await page.setViewportSize(WINDOW);
  await page.emulateMedia({ media: "screen", colorScheme: theme as "light" });
  await page.goto(
    `/docs/manual/screenshots/app.html?lang=${language}&theme=${theme}`,
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
  // Let the window grow until its settings no longer scroll, so the PDF
  // (one page, the window's size) holds all of them. The scrollbars are
  // overlays, as on macOS, so the width of the content stays the same.
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

/** The marked part of a shot (`data-manual-shot`). */
const SHOT_SELECTOR = "[data-manual-shot]";

/**
 * Marks the settings group titled `title` as the shot and leaves only what
 * the picture shows visible: the group, of its list only the first `rows`
 * rows and the next one (to fade out), with the window's background. The
 * layout stays as it is, so the PDF holds nothing else. Returns the part's
 * box in CSS pixels, where the list is cut, and the window's background.
 */
const prepareShot = (page: Page, title: string, rows: number | undefined) =>
  page.evaluate(
    ([wanted, shown]) => {
      const heading = [...document.querySelectorAll("h2")].find(
        (element) => element.textContent?.trim() === wanted,
      );
      // SettingsGroup: the title's block, then the card, in one element.
      const group = heading?.parentElement?.parentElement;
      if (!group) throw new Error(`no settings group "${wanted}"`);
      group.setAttribute("data-manual-shot", "");
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
      if (shown !== null) {
        // A row of the list: the element around one end's name field whose
        // parent holds the others too.
        const names = group.querySelectorAll('[id^="end-key-name-"]');
        const rowOf = (field: Element) => {
          let row = field;
          while (
            row.parentElement &&
            row.parentElement.querySelectorAll('[id^="end-key-name-"]')
              .length === 1
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
        cut,
        background: getComputedStyle(document.documentElement).backgroundColor,
      };
    },
    [title, rows ?? null] as const,
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
 * last part fading into the window's background, clip paths merged, and
 * svgo's safe default optimizations. */
const finishSvg = (
  raw: string,
  widthPt: number,
  heightPt: number,
  fade: { heightPt: number; color: string } | null,
): string => {
  const sized = sizeSvg(raw, widthPt, heightPt);
  const faded = fade
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
        await openWindow(page, language, theme);
        const viewport = page.viewportSize();
        if (!viewport) throw new Error("no viewport");
        const title = translation(language, shot.groupTitleKey);
        const preview =
          PREVIEW && join(PREVIEW, `${shot.name}.${language}.${theme}`);
        if (preview) {
          // The window as the browser shows it, before anything is hidden.
          mkdirSync(PREVIEW as string, { recursive: true });
          await page.screenshot({ path: `${preview}.window.png` });
        }
        const { box, cut, background } = await prepareShot(
          page,
          title,
          shot.rows,
        );
        // Open fonts only, on any machine (`boot.ts`).
        const fonts = await fontsOfShot(page);
        expect(fonts).toContain("Inter");
        expect(fonts.filter((family) => !FONTS.includes(family))).toEqual([]);
        // The part with its margin, or down to the cut and its fade, in
        // points (pdftocairo's unit), snapped outward to the grid.
        const snap = (px: number, round: (n: number) => number) =>
          round((px * PT_PER_PX) / GRID_PT) * GRID_PT;
        const left = snap(box.x - MARGIN, Math.floor);
        const top = snap(box.y - MARGIN, Math.floor);
        const right = snap(box.x + box.width + MARGIN, Math.ceil);
        const bottom =
          cut === null
            ? snap(box.y + box.height + MARGIN, Math.ceil)
            : snap(cut + FADE, Math.ceil);
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
          const svg = finishSvg(
            raw,
            width,
            height,
            fadeHeight === null
              ? null
              : { heightPt: fadeHeight, color: background },
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
