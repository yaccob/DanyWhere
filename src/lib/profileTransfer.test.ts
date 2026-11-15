import assert from "node:assert/strict";
import {
  checkNewPassword,
  exportFileName,
  transferErrorKey,
} from "./profileTransfer";

let failed = 0;
const check = (label: string, actual: unknown, expected: unknown) => {
  try {
    assert.deepEqual(actual, expected);
  } catch {
    failed++;
    console.error(
      `${label}: got ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`,
    );
  }
};

// Passwords: no rules beyond "not empty" and "typed twice alike".
check("empty", checkNewPassword("", ""), "empty");
check("only spaces", checkNewPassword("   ", "   "), "empty");
check("mismatch", checkNewPassword("geheim", "geheim1"), "mismatch");
check("short is fine", checkNewPassword("a", "a"), null);
check(
  "long is fine",
  checkNewPassword("ein langer Satz", "ein langer Satz"),
  null,
);

// File names: the profile's name, safe on every platform, with .yaml.
check("plain", exportFileName("Work"), "Work.yaml");
check("umlauts stay", exportFileName("Büro"), "Büro.yaml");
check("slashes", exportFileName("Mail/Chat"), "Mail-Chat.yaml");
check("windows chars", exportFileName('a:b*c?"<>|'), "a-b-c-.yaml");
check("trimmed", exportFileName("  Work  "), "Work.yaml");
check("empty", exportFileName("   "), "profile.yaml");
check("dots only", exportFileName(".."), "profile.yaml");

// Every error code has its own text.
const codes = [
  "profile_not_found",
  "password_required",
  "empty_password",
  "wrong_password",
  "unsupported_format",
  "malformed",
  "io",
  "internal",
] as const;
const keys = codes.map(transferErrorKey);
check("distinct keys", new Set(keys).size, codes.length);
for (const key of keys) {
  check(`key ${key}`, key.startsWith("settings.profileTransfer.errors."), true);
}

assert.equal(failed, 0, `${failed} checks failed`);
console.log("profileTransfer: all assertions passed");
