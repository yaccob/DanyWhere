#!/usr/bin/env bash
# Builds DanyWhere's input method (InputMethodKit, Objective-C) as a signed,
# universal (arm64 + x86_64) bundle for macOS 10.15 and later, or runs its
# logic tests. Needs only the Command Line Tools (clang, codesign, plutil,
# lipo); no Xcode, no XCTest. Names and ids follow PROTOCOL.md.
#
#   build.sh --app-id <app bundle id> --name "<product name>" --version <x.y.z> \
#            --identity "<codesign identity, or - for ad hoc>" [--hardened] --out <dir>
#   build.sh --test [--out <dir>]
#   build.sh --make-icon
#
# Build: writes "<dir>/<name> Input.app" (bundle id
# "<app id>.inputmethod.DanyWhere"), signs it (--hardened adds the hardened
# runtime and a secure timestamp, as notarization needs), verifies it with
# `codesign --verify --strict` and prints its path as the last line on stdout.
# Everything else goes to stderr, with "== phase: <name> (HH:MM:SS)" lines:
# compile, bundle, sign, verify, done.
#
# Test: compiles every source but main.m with tests/*.m into a test runner in
# <dir> (default ~/Library/Caches/danywhere/input-method/test), signs it with
# "DanyWhere Local Signing" when that identity is in the keychain, so that the
# real peer check runs against a real certificate (DW_TEST_IDENTITY names
# another identity, "-" keeps it ad hoc and skips that test), and runs it
# natively and, where Rosetta is installed, as x86_64 as well. Exits non-zero
# when a test fails. Phases: compile-tests, test-<arch>, done.
#
# --make-icon: redraws Icon/DanyWhereInput.tiff (committed) from Icon/make-icon.m.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
min_os=10.15
archs=(-arch arm64 -arch x86_64)
exec_name=DanyWhereInput
icon_name=DanyWhereInput.tiff
frameworks=(-framework Foundation -framework AppKit -framework InputMethodKit -framework Carbon -framework Security)
cflags=(-fobjc-arc -mmacosx-version-min="$min_os" -Wall -Wextra -Wno-unused-parameter -Werror -I"$here/Sources")

phase() {
  echo "== phase: $1 ($(date '+%H:%M:%S'))" >&2
}

die() {
  echo "build.sh: $*" >&2
  exit 1
}

usage() {
  sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

mode=build
app_id=""
name=""
version=""
identity=""
hardened=false
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --app-id) app_id=${2:-}; shift 2 ;;
    --name) name=${2:-}; shift 2 ;;
    --version) version=${2:-}; shift 2 ;;
    --identity) identity=${2:-}; shift 2 ;;
    --hardened) hardened=true; shift ;;
    --out) out=${2:-}; shift 2 ;;
    --test) mode=test; shift ;;
    --make-icon) mode=icon; shift ;;
    -h|--help) usage ;;
    *) echo "build.sh: unknown argument: $1" >&2; usage ;;
  esac
done

work=$(mktemp -d "${TMPDIR:-/tmp}/danywhere-input-method.XXXXXX")
trap 'rm -rf "$work"' EXIT

# Every source but main.m: the logic and the IMK controller, for the tests.
library_sources=()
for source in "$here"/Sources/*.m; do
  [ "$(basename "$source")" = main.m ] || library_sources+=("$source")
done

if [ "$mode" = icon ]; then
  phase make-icon
  clang -fobjc-arc -mmacosx-version-min="$min_os" -framework AppKit "$here/Icon/make-icon.m" -o "$work/make-icon"
  "$work/make-icon" "$here/Icon/$icon_name"
  phase done
  echo "$here/Icon/$icon_name"
  exit 0
fi

if [ "$mode" = test ]; then
  out=${out:-$HOME/Library/Caches/danywhere/input-method/test}
  mkdir -p "$out"
  runner="$out/input-method-tests"
  phase compile-tests
  clang "${cflags[@]}" "${archs[@]}" -O1 -I"$here/tests" "${frameworks[@]}" \
    "${library_sources[@]}" "$here"/tests/*.m -o "$runner"
  lipo -archs "$runner" >&2

  test_identity=${DW_TEST_IDENTITY:-DanyWhere Local Signing}
  signed_id=danywhere.inputmethod.tests
  if [ "$test_identity" != "-" ] && security find-identity -v -p codesigning | grep -qF "\"$test_identity\""; then
    codesign --force --sign "$test_identity" --identifier "$signed_id" "$runner"
    export DW_TEST_SIGNED_ID="$signed_id"
    echo "Test runner signed with \"$test_identity\" as $signed_id" >&2
  else
    unset DW_TEST_SIGNED_ID || true
    echo "Test runner stays ad hoc: the real-signature test is skipped" >&2
  fi

  failed=0
  phase "test-$(uname -m)"
  "$runner" || failed=1
  if [ "$(uname -m)" = arm64 ] && /usr/bin/arch -x86_64 /usr/bin/true 2>/dev/null; then
    phase test-x86_64
    /usr/bin/arch -x86_64 "$runner" || failed=1
  else
    echo "(no Rosetta: x86_64 slice compiled but not run)" >&2
  fi
  phase done
  if [ "$failed" = 1 ]; then
    echo "TESTS FAILED" >&2
    exit 1
  fi
  echo "All input method tests passed" >&2
  exit 0
fi

# --- Build ----------------------------------------------------------------------
[ -n "$app_id" ] && [ -n "$name" ] && [ -n "$version" ] && [ -n "$identity" ] && [ -n "$out" ] || usage
[[ "$app_id" =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]] || die "--app-id is not a bundle id: $app_id"
[[ "$version" =~ ^[0-9]+(\.[0-9]+)*([-+][0-9A-Za-z.-]+)?$ ]] || die "--version is not a version: $version"
case "$name" in */*|.*) die "--name must not contain / or start with a dot: $name" ;; esac

im_id="$app_id.inputmethod.DanyWhere"
bundle_name="$name Input.app"
mkdir -p "$out"
out=$(cd "$out" && pwd)
final="$out/$bundle_name"
staged="$out/.$bundle_name.building"
rm -rf "$staged"

phase compile
clang "${cflags[@]}" "${archs[@]}" -O2 "${frameworks[@]}" "$here"/Sources/*.m -o "$work/$exec_name"
lipo -archs "$work/$exec_name" >&2

phase bundle
mkdir -p "$staged/Contents/MacOS" "$staged/Contents/Resources"
cp "$work/$exec_name" "$staged/Contents/MacOS/$exec_name"
cp "$here/Icon/$icon_name" "$staged/Contents/Resources/$icon_name"
printf 'APPL????' > "$staged/Contents/PkgInfo"
plist="$staged/Contents/Info.plist"
cp "$here/Info.plist" "$plist"
plutil -replace CFBundleIdentifier -string "$im_id" "$plist"
plutil -replace TISInputSourceID -string "$im_id" "$plist"
plutil -replace InputMethodConnectionName -string "${im_id}_Connection" "$plist"
plutil -replace CFBundleName -string "$name" "$plist"
plutil -replace CFBundleDisplayName -string "$name" "$plist"
plutil -replace CFBundleShortVersionString -string "$version" "$plist"
plutil -replace CFBundleVersion -string "$version" "$plist"
plutil -replace DanyWhereAppID -string "$app_id" "$plist"
if grep -q '__[A-Z_]*__' "$plist"; then
  die "placeholders left in Info.plist: $(grep -o '__[A-Z_]*__' "$plist" | sort -u | tr '\n' ' ')"
fi
plutil -lint "$plist" >&2

phase sign
sign_args=(--force --sign "$identity")
if $hardened; then
  sign_args+=(--options runtime --timestamp)
fi
codesign "${sign_args[@]}" "$staged"

phase verify
codesign --verify --strict --verbose=2 "$staged"
codesign -dv "$staged" 2>&1 | grep -E '^(Identifier|Format|CodeDirectory|Signature|Authority|TeamIdentifier|Timestamp|Runtime)' >&2 || true
rm -rf "$final"
mv "$staged" "$final"
phase done
echo "$final"
