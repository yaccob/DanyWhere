// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/components/settings/PostProcessingSettingsApi/effortChoice.test.ts
import assert from "node:assert";
import type { EffortOptions } from "@/bindings";
import { effortChoice, MODEL_DEFAULT_EFFORT } from "./effortChoice";

let failures = 0;
let checks = 0;
const check = (name: string, actual: unknown, expected: unknown) => {
  checks += 1;
  try {
    assert.deepStrictEqual(actual, expected);
  } catch {
    failures += 1;
    console.error(
      `FAIL ${name}: got ${JSON.stringify(actual)}, want ${JSON.stringify(expected)}`,
    );
  }
};

const listed: EffortOptions = {
  levels: ["low", "medium", "high", "xhigh", "max"],
  source: "listed",
  rejected: [],
};

// The model's default comes first, then the model's levels.
check("levels of a listed model", effortChoice(listed, null).values, [
  MODEL_DEFAULT_EFFORT,
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
]);
check(
  "nothing stored shows the default",
  effortChoice(listed, null).selected,
  "",
);
check("a stored level shows", effortChoice(listed, "xhigh").selected, "xhigh");

// A level the new model is known not to take falls back to the default.
const withoutXhigh: EffortOptions = {
  levels: ["low", "medium", "high", "max"],
  source: "documented",
  rejected: [],
};
check(
  "a level the model lacks shows the default",
  effortChoice(withoutXhigh, "xhigh").selected,
  MODEL_DEFAULT_EFFORT,
);
check(
  "and isn't offered",
  effortChoice(withoutXhigh, "xhigh").values.includes("xhigh"),
  false,
);

// A model known to take no effort offers only its default.
const none: EffortOptions = { levels: [], source: "listed", rejected: [] };
check("no effort: only the default", effortChoice(none, "high").values, [""]);
check("no effort: says so", effortChoice(none, "high").noEffort, true);
check("no effort: shows the default", effortChoice(none, "high").selected, "");
check("levels known: not a guess", effortChoice(listed, null).guessed, false);

// Nothing known: the common levels, and a stored other level stays visible,
// since the model may well take it.
const common: EffortOptions = {
  levels: ["low", "medium", "high"],
  source: "common",
  rejected: [],
};
check("unknown model is a guess", effortChoice(common, null).guessed, true);
check(
  "unknown model keeps a stored level in order",
  effortChoice(common, "xhigh").values,
  ["", "low", "medium", "high", "xhigh"],
);
check(
  "unknown model shows the stored level",
  effortChoice(common, "xhigh").selected,
  "xhigh",
);
check(
  "unknown model: not 'no effort'",
  effortChoice(common, null).noEffort,
  false,
);
check(
  "an unknown value stays hidden",
  effortChoice(common, "turbo").selected,
  "",
);

// A level the endpoint refused is shown with a hint.
const refused: EffortOptions = { ...common, rejected: ["high"] };
check("refused level", effortChoice(refused, "high").refused, true);
check("other level", effortChoice(refused, "low").refused, false);
check("default is never refused", effortChoice(refused, null).refused, false);

// Before the options arrive, a stored level is kept visible.
check("loading keeps the stored level", effortChoice(null, "high").values, [
  "",
  "high",
]);
check(
  "loading shows the stored level",
  effortChoice(null, "high").selected,
  "high",
);
check("loading without one", effortChoice(null, null).values, [""]);

console.log(`${checks - failures}/${checks} checks passed`);
if (failures > 0) {
  process.exit(1);
}
