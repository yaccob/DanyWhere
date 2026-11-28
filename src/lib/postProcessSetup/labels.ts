// DanyWhere: numbers and names the post-processing setup shows, in the
// app's language, without translations of their own (Intl).

/** The plans of a subscription as one choice: the first in full, the others
 * without the words they share with it ("ChatGPT Go, Plus, Pro, or
 * Business"; in German "ChatGPT Go, Plus, Pro oder Business"). */
export const plansLabel = (plans: readonly string[], language: string) => {
  if (plans.length < 2) return plans[0] ?? "";
  const words = plans.map((plan) => plan.split(" "));
  let shared = 0;
  while (
    shared < words[0].length - 1 &&
    words.every((w) => w.length - 1 > shared && w[shared] === words[0][shared])
  ) {
    shared += 1;
  }
  const rest = words.slice(1).map((w) => w.slice(shared).join(" "));
  return new Intl.ListFormat(language, {
    style: "long",
    type: "disjunction",
  }).format([plans[0], ...rest]);
};

/** Memory in whole gigabytes ("8 GB"), as the questions name 16 GB. */
export const memoryLabel = (bytes: number, language: string) =>
  new Intl.NumberFormat(language, {
    style: "unit",
    unit: "gigabyte",
    maximumFractionDigits: 0,
  }).format(Math.round(bytes / 1024 ** 3));

/** Seconds with one decimal below ten ("7.3"), whole above. */
export const secondsLabel = (millis: number, language: string) => {
  const seconds = millis / 1000;
  const digits = seconds < 10 ? 1 : 0;
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(seconds);
};

/** US dollars, as OpenRouter's limits are ("$10.00"). */
export const dollarsLabel = (usd: number, language: string) =>
  new Intl.NumberFormat(language, {
    style: "currency",
    currency: "USD",
  }).format(usd);
