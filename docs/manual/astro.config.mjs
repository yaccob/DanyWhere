// @ts-check
import { fileURLToPath } from "node:url";
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// One source, two outputs. MANUAL_TARGET picks which one this build makes:
// - "app" (default): served by the DanyWhere app at /manual/… The output goes
//   to the repo root's public/manual/, so Vite copies it into dist/manual/ and
//   Tauri embeds it with the rest of the frontend.
// - "web": the site for GitHub Pages at https://yaccob.github.io/DanyWhere/.
// scripts/manual.ts sets it; see the root package.json's *:manual scripts.
const target = process.env.MANUAL_TARGET === "web" ? "web" : "app";

const outDir =
  target === "web"
    ? fileURLToPath(new URL("./dist/", import.meta.url))
    : fileURLToPath(new URL("../../public/manual/", import.meta.url));

export default defineConfig({
  // Only the web build has a public address; without it the app build gets no
  // canonical links and no sitemap pointing somewhere else.
  site: target === "web" ? "https://yaccob.github.io" : undefined,
  base: target === "web" ? "/DanyWhere" : "/manual",
  outDir,
  // Every page is <slug>/index.html and every link ends in a slash. Tauri's
  // asset resolver maps /manual/foo/ to manual/foo/index.html, and relative
  // links inside a page only resolve as intended when its URL ends in a slash.
  trailingSlash: "always",
  build: { format: "directory" },
  vite: {
    define: {
      // Components check at build time which language's image exists.
      __MANUAL_PUBLIC_DIR__: JSON.stringify(
        fileURLToPath(new URL("./public/", import.meta.url)),
      ),
    },
  },
  integrations: [
    starlight({
      title: {
        en: "DanyWhere Manual",
        de: "DanyWhere-Handbuch",
        es: "Manual de DanyWhere",
        fr: "Manuel de DanyWhere",
      },
      description: "How to use DanyWhere, the speech-to-text app.",
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        de: { label: "Deutsch", lang: "de" },
        es: { label: "Español", lang: "es" },
        fr: { label: "Français", lang: "fr" },
      },
      logo: {
        light: "./src/assets/wordmark-light.svg",
        dark: "./src/assets/wordmark-dark.svg",
        alt: "DanyWhere",
        replacesTitle: true,
      },
      favicon: "/favicon.svg",
      customCss: ["./src/styles/theme.css", "./src/styles/figures.css"],
      components: {
        SiteTitle: "./src/components/SiteTitle.astro",
      },
      sidebar: [
        {
          label: "How DanyWhere works",
          translations: {
            de: "So arbeitet DanyWhere",
            es: "Cómo funciona DanyWhere",
            fr: "Comment fonctionne DanyWhere",
          },
          items: [{ autogenerate: { directory: "how-it-works" } }],
        },
        // Origin, thanks and the repository; last, after every chapter.
        {
          label: "About DanyWhere",
          translations: {
            de: "Über DanyWhere",
            es: "Acerca de DanyWhere",
            fr: "À propos de DanyWhere",
          },
          slug: "about",
        },
      ],
      lastUpdated: false,
      credits: false,
    }),
  ],
});
