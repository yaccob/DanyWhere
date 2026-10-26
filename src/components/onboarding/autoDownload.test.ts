import assert from "node:assert/strict";
import type { ModelInfo } from "@/bindings";
import { autoDownloadCandidate, modelToFollow } from "./autoDownload";

// Only the fields the rule looks at; the rest of ModelInfo is irrelevant here.
const model = (
  id: string,
  opts: { downloaded?: boolean; recommended?: boolean; legacy?: boolean } = {},
): ModelInfo =>
  ({
    id,
    is_downloaded: opts.downloaded ?? false,
    is_recommended: opts.recommended ?? false,
    source: opts.legacy
      ? { Url: { url: "https://example.invalid/m.bin", sha256: null } }
      : { HuggingFace: { repo_id: `org/${id}`, revision: "abc" } },
  }) as unknown as ModelInfo;

const turbo = model("turbo", { recommended: true });
const parakeet = model("parakeet", { recommended: true });
const small = model("small");

// A fresh install downloads the first recommended model (models arrive in
// rank order) on its own.
assert.equal(autoDownloadCandidate([turbo, parakeet, small], {}), "turbo");

// Legacy (.bin) downloads are never picked, even when listed first.
assert.equal(
  autoDownloadCandidate(
    [model("old", { recommended: true, legacy: true }), turbo],
    {},
  ),
  "turbo",
);

// A model on disk means the user already has something to choose: no download.
assert.equal(
  autoDownloadCandidate([turbo, model("small", { downloaded: true })], {}),
  null,
);

// A download in progress (e.g. started before a reload) is not doubled.
assert.equal(
  autoDownloadCandidate([turbo, parakeet], { parakeet: true }),
  null,
);

// Nothing recommended: no automatic download.
assert.equal(autoDownloadCandidate([small], {}), null);

// The model step follows the download started during the permission step:
// still running, being verified, or already finished.
assert.equal(modelToFollow("turbo", [turbo], { turbo: true }, {}), "turbo");
assert.equal(modelToFollow("turbo", [turbo], {}, { turbo: true }), "turbo");
assert.equal(
  modelToFollow("turbo", [model("turbo", { downloaded: true })], {}, {}),
  "turbo",
);

// A download that failed or was cancelled is not followed: the user picks.
assert.equal(modelToFollow("turbo", [turbo], {}, {}), null);

// Nothing was started automatically: nothing to follow.
assert.equal(modelToFollow(null, [turbo], { turbo: true }, {}), null);

console.log("autoDownload: all assertions passed");
