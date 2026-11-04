import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { modelMatchesLanguage } from "@handy-computer/models";
import { CURATED_ASR, picksFor, type MachineTier } from "./catalog";
import { withFallbackPick } from "./fallbackPick";

// DanyWhere's offer, read from the backend's selection so the test follows it.
const selection = readFileSync(
  new URL("../../../src-tauri/src/model_selection.toml", import.meta.url),
  "utf8",
);
const offeredIds = new Set(
  [...selection.matchAll(/^id\s*=\s*"([^"]+)"/gm)].map((m) => m[1]),
);
const offered = CURATED_ASR.filter((m) => offeredIds.has(m.id));
assert.ok(offered.length >= 2, "model_selection.toml lists offered models");
const isOffered = (m: { id: string }) => offeredIds.has(m.id);

const failures: string[] = [];
const check = (ok: boolean, what: string) => {
  if (!ok) failures.push(what);
};

const tiers: MachineTier[] = ["accelerated", "cpu"];
const cases: string[][] = [["de", "en"], ["de"], ["fr"], ["en"], ["zu"]];

for (const languages of cases) {
  for (const tier of tiers) {
    const label = `${languages.join("+")} on ${tier}`;
    const picks = withFallbackPick(
      picksFor(languages, isOffered),
      languages,
      tier,
      offered,
    );
    const recommended = picks.filter((p) => p.role === "pick");
    // Onboarding's model step reads the "Recommended" pick unguarded.
    check(recommended.length === 1, `${label}: exactly one pick`);
    check(
      picks.every((p) => isOffered(p.model)),
      `${label}: only offered models are picked`,
    );
    check(
      new Set(picks.map((p) => p.model.id)).size === picks.length,
      `${label}: no model appears twice`,
    );
  }
}

// A pick that speaks every chosen language when an offered model does.
for (const languages of [["de", "en"], ["de"], ["fr"]]) {
  const [pick] = withFallbackPick(
    picksFor(languages, isOffered),
    languages,
    "accelerated",
    offered,
  ).filter((p) => p.role === "pick");
  check(
    !!pick && languages.every((l) => modelMatchesLanguage(pick.model, l)),
    `${languages.join("+")}: the pick speaks all chosen languages`,
  );
}

// The package's own pick stays when DanyWhere offers it.
const englishPackage = picksFor(["en"], isOffered);
const english = withFallbackPick(
  englishPackage,
  ["en"],
  "accelerated",
  offered,
);
check(
  english.find((p) => p.role === "pick")?.model.id ===
    englishPackage.find((p) => p.role === "pick")?.model.id,
  "en: the package's pick is kept",
);

// Nothing offered (e.g. before the model list has loaded): nothing invented.
check(
  withFallbackPick([], ["de"], "accelerated", []).length === 0,
  "nothing offered: no pick",
);

assert.equal(
  failures.length,
  0,
  `${failures.length} check(s) failed:\n  ${failures.join("\n  ")}`,
);
console.log("fallbackPick: all checks passed");
