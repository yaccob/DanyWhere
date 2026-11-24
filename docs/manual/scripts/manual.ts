// Builds or serves the manual; the root package.json's *:manual scripts call it.
//
//   bun docs/manual/scripts/manual.ts build app   -> public/manual/ (base /manual)
//   bun docs/manual/scripts/manual.ts build web   -> docs/manual/dist/ (base /DanyWhere)
//   bun docs/manual/scripts/manual.ts dev         -> http://localhost:4321/manual/
//
// It installs docs/manual's dependencies from its lockfile first when they are
// missing or older than the lockfile, and runs Astro on Bun, so the build needs
// no particular Node version (Astro 7 wants Node 22.23.4 or newer).
//
// DANYWHERE_SKIP_MANUAL=1 skips the app build, for builds without network
// access to docs/manual's dependencies (such as the Nix sandbox); the app then
// ships without the manual.

import { spawnSync } from "node:child_process";
import { existsSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const manualDir = join(import.meta.dir, "..");
const repoDir = join(manualDir, "..", "..");
const bun = process.execPath;

const [command = "build", target = "app"] = process.argv.slice(2);
if (
  !["build", "dev", "preview"].includes(command) ||
  !["app", "web"].includes(target)
) {
  console.error("usage: manual.ts <build|dev|preview> [app|web]");
  process.exit(2);
}

if (
  command === "build" &&
  target === "app" &&
  process.env.DANYWHERE_SKIP_MANUAL === "1"
) {
  console.log(
    "[manual] DANYWHERE_SKIP_MANUAL=1: the app ships without the manual",
  );
  process.exit(0);
}

function run(args: string[], env: Record<string, string> = {}): void {
  const result = spawnSync(bun, args, {
    cwd: manualDir,
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function ensureDependencies(): void {
  const stamp = join(manualDir, "node_modules", ".installed-from-lockfile");
  const lockfile = join(manualDir, "bun.lock");
  const fresh =
    existsSync(join(manualDir, "node_modules", "astro")) &&
    existsSync(stamp) &&
    statSync(stamp).mtimeMs >= statSync(lockfile).mtimeMs;
  if (fresh) return;
  console.log(
    "[manual] installing docs/manual's dependencies (bun install --frozen-lockfile)",
  );
  run(["install", "--frozen-lockfile"]);
  writeFileSync(stamp, "");
}

ensureDependencies();

const outDir =
  target === "web"
    ? join(manualDir, "dist")
    : join(repoDir, "public", "manual");
if (command === "build") {
  // Astro does not empty an outDir outside its project; a page removed from
  // the sources must not linger in the app.
  rmSync(outDir, { recursive: true, force: true });
}

run(["--bun", "astro", command], {
  MANUAL_TARGET: target,
  ASTRO_TELEMETRY_DISABLED: "1",
});

if (command === "build" && target === "web") {
  // GitHub Pages' Jekyll step would drop the _astro/ folder.
  writeFileSync(join(outDir, ".nojekyll"), "");
}
if (command === "build") console.log(`[manual] ${target} build in ${outDir}`);
