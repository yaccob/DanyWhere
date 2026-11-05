import assert from "node:assert/strict";
import { autoDownloadPick, type AutoDownloadInput } from "./autoDownload";

const pick = (id: string, role = "pick") => ({ model: { id }, role });
const fresh: AutoDownloadInput = {
  picks: [pick("parakeet-ultra"), pick("whisper-small", "faster")],
  states: ["none", "none", "none"],
  chosen: null,
};

const failures: string[] = [];
const check = (got: string | null, want: string | null, what: string) => {
  if (got !== want) failures.push(`${what}: got ${got}, want ${want}`);
};

// A fresh install starts the recommended model for the chosen languages, so
// it loads while the user grants permissions.
check(autoDownloadPick(fresh), "parakeet-ultra", "fresh install");

// Only the "Recommended" pick, never a "faster" or "more accurate" one.
check(
  autoDownloadPick({
    ...fresh,
    picks: [pick("canary-1b-v2"), pick("x", "faster")].reverse(),
  }),
  "canary-1b-v2",
  "pick found among other roles",
);
check(
  autoDownloadPick({ ...fresh, picks: [pick("whisper-small", "faster")] }),
  null,
  "no recommended pick",
);

// Models not loaded yet: nothing to start.
check(autoDownloadPick({ ...fresh, picks: [] }), null, "no picks yet");

// A model on disk, a running download or one being checked: nothing doubled.
check(
  autoDownloadPick({ ...fresh, states: ["none", "ready"] }),
  null,
  "model on disk",
);
check(
  autoDownloadPick({ ...fresh, states: ["downloading", "none"] }),
  null,
  "download running",
);
check(
  autoDownloadPick({ ...fresh, states: ["checking"] }),
  null,
  "download being checked",
);

// A failed earlier attempt does not block a new start.
check(
  autoDownloadPick({ ...fresh, states: ["failed", "none"] }),
  "parakeet-ultra",
  "after a failed download",
);

// The user (or a resumed onboarding) already has a model: leave it.
check(
  autoDownloadPick({ ...fresh, chosen: "whisper-small" }),
  null,
  "already chosen",
);

assert.equal(
  failures.length,
  0,
  `${failures.length} check(s) failed:\n  ${failures.join("\n  ")}`,
);
console.log("autoDownload: all checks passed");
