// Finds the files behind <Diagram> and <Screenshot> at build time:
// public/<kind>/<lang>/<name>.light.svg and .dark.svg, in the page's language
// or, when that language has none, in English.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export type MediaKind = "diagrams" | "screenshots";
export type Theme = "light" | "dark";

export interface MediaFile {
  /** URL of the file, with the site's base. */
  src: string;
  /** The language folder the file came from. */
  lang: string;
  /** Intrinsic size from the SVG, so the page reserves its space. */
  width?: number;
  height?: number;
}

export interface ResolvedMedia {
  light?: MediaFile;
  dark?: MediaFile;
  /** Every path that was looked for, for the warning when nothing exists. */
  tried: string[];
}

export const FALLBACK_LANG = "en";
const NAME = /^[a-z0-9][a-z0-9-]*$/;
const BASE = import.meta.env.BASE_URL.replace(/\/$/, "");

/** Width and height of an SVG: its width/height attributes, else its viewBox. */
function svgSize(file: string): { width?: number; height?: number } {
  const head = readFileSync(file, "utf8").match(/<svg\b[^>]*>/)?.[0] ?? "";
  const attr = (name: string) =>
    head.match(new RegExp(`\\s${name}="([\\d.]+)(px)?"`))?.[1];
  const width = attr("width");
  const height = attr("height");
  if (width && height) return { width: +width, height: +height };
  const viewBox = head
    .match(/\sviewBox="([^"]+)"/)?.[1]
    ?.trim()
    .split(/[\s,]+/);
  if (viewBox?.length === 4) return { width: +viewBox[2], height: +viewBox[3] };
  return {};
}

export function resolveMedia(
  kind: MediaKind,
  name: string,
  locale: string | undefined,
): ResolvedMedia {
  if (!NAME.test(name)) {
    throw new Error(
      `[manual] ${kind} name "${name}" must be lower-case letters, digits and dashes`,
    );
  }
  const lang = (locale ?? FALLBACK_LANG).toLowerCase();
  const langs = lang === FALLBACK_LANG ? [lang] : [lang, FALLBACK_LANG];
  const tried: string[] = [];

  const find = (theme: Theme): MediaFile | undefined => {
    for (const l of langs) {
      const rel = `${kind}/${l}/${name}.${theme}.svg`;
      const file = join(__MANUAL_PUBLIC_DIR__, rel);
      tried.push(`public/${rel}`);
      if (existsSync(file)) {
        return { src: `${BASE}/${rel}`, lang: l, ...svgSize(file) };
      }
    }
    return undefined;
  };

  const light = find("light");
  const dark = find("dark");
  // One theme's file stands in for the other's when only one exists.
  return { light: light ?? dark, dark: dark ?? light, tried };
}
