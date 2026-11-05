import assert from "node:assert/strict";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Codes } from "./columns";

// The app's webview shows no native `title` tooltips (seen 2026-11-05:
// hovering over or clicking the speed and size icons showed nothing). Every
// column head therefore explains itself through the app's own Tooltip, which
// also opens on a click; its anchor is an inline-grid span.
for (const langs of [
  ["de", "en"],
  ["de", "en", "fr", "es"],
  ["de", "en", "fr", "es", "it", "pt"],
]) {
  const html = renderToStaticMarkup(React.createElement(Codes, { langs }));
  assert.doesNotMatch(
    html,
    /\stitle="/,
    `${langs.length} languages: a head relies on a native title tooltip`,
  );
  const heads = langs.length > 5 ? 3 : langs.length + 2;
  assert.equal(
    (html.match(/class="inline-grid/g) ?? []).length,
    heads,
    `${langs.length} languages: every head has the app's tooltip`,
  );
}
