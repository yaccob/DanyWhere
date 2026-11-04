// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/constants/branding.test.ts
import assert from "node:assert";
import { SOURCE_CODE_URL, UPSTREAM_URL } from "./branding";

// "Source Code" on the About page shows this distribution's code, not Handy's.
assert.strictEqual(SOURCE_CODE_URL, "https://github.com/yaccob/DanyWhere");

// Credits link to the upstream project.
assert.strictEqual(UPSTREAM_URL, "https://github.com/cjpais/Handy");

console.log("branding constants: all checks passed");
