import { defineCollection } from "astro:content";
import { z } from "astro/zod";
import { docsLoader, i18nLoader } from "@astrojs/starlight/loaders";
import { docsSchema, i18nSchema } from "@astrojs/starlight/schema";

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  // The manual's own UI strings, next to Starlight's (src/content/i18n/).
  i18n: defineCollection({
    loader: i18nLoader(),
    schema: i18nSchema({
      extend: z
        .object({
          "dw.siteTitle.label": z.string(),
          "dw.figure.zoom": z.string(),
          "dw.figure.close": z.string(),
          "dw.figure.missing": z.string(),
        })
        .partial(),
    }),
  }),
};
