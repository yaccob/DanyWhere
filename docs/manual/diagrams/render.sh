#!/usr/bin/env bash
# Renders the manual's diagrams with PlantUML.
#
# Every docs/manual/diagrams/<name>.<lang>.puml becomes
# docs/manual/public/diagrams/<lang>/<name>.light.svg and .dark.svg; the
# theme reaches the source as THEME (style.iuml picks its colours by it).
#
#   render.sh           render all diagrams
#   render.sh --check   render into a temporary folder and fail when a
#                       committed SVG is missing, stale or has no source
#
# PlantUML: `plantuml` on PATH, or the command in $PLANTUML. SVG only.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
public="$(cd "$here/.." && pwd)/public/diagrams"
plantuml="${PLANTUML:-plantuml}"
themes=(light dark)

check=false
case "${1:-}" in
  "") ;;
  --check) check=true ;;
  *)
    echo "usage: $(basename "$0") [--check]" >&2
    exit 2
    ;;
esac

if ! command -v "$plantuml" >/dev/null 2>&1; then
  echo "PlantUML not found: install it or set PLANTUML" >&2
  exit 2
fi

shopt -s nullglob
sources=("$here"/*.*.puml)
if [ ${#sources[@]} -eq 0 ]; then
  echo "No diagram sources (<name>.<lang>.puml) in $here" >&2
  exit 2
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The SVG names Arial, the font PlantUML measured the text in; browsers get
# it with metric-compatible fallbacks, so the text keeps its width.
font_stack="Arial, Helvetica, 'Liberation Sans', 'Arimo', sans-serif"

# Renders every source in one PlantUML run per theme, then moves each SVG
# to <dest>/<lang>/<name>.<theme>.svg.
render_into() {
  local dest="$1" theme src base name lang raw
  for theme in "${themes[@]}"; do
    mkdir -p "$work/raw-$theme"
    echo "Rendering ${#sources[@]} diagram(s), $theme"
    "$plantuml" -tsvg -charset UTF-8 -failfast2 -DTHEME="$theme" \
      -o "$work/raw-$theme" "${sources[@]}"
    for src in "${sources[@]}"; do
      base="$(basename "$src" .puml)"
      name="${base%.*}"
      lang="${base##*.}"
      raw="$work/raw-$theme/$base.svg"
      if [ ! -f "$raw" ]; then
        echo "PlantUML wrote no SVG for $base ($theme)" >&2
        exit 1
      fi
      mkdir -p "$dest/$lang"
      sed -e "s/font-family=\"Arial\"/font-family=\"$font_stack\"/g" \
        "$raw" >"$dest/$lang/$name.$theme.svg"
    done
  done
}

if ! $check; then
  render_into "$public"
  echo "Done: $public"
  exit 0
fi

render_into "$work/fresh"
failed=0
for fresh in "$work"/fresh/*/*.svg; do
  rel="${fresh#"$work/fresh/"}"
  if [ ! -f "$public/$rel" ]; then
    echo "missing: $rel"
    failed=1
  elif ! cmp -s "$fresh" "$public/$rel"; then
    echo "stale:   $rel"
    failed=1
  fi
done
for committed in "$public"/*/*.svg; do
  rel="${committed#"$public/"}"
  if [ ! -f "$work/fresh/$rel" ]; then
    echo "no source: $rel"
    failed=1
  fi
done
if [ "$failed" -ne 0 ]; then
  echo "Diagrams are out of date: run docs/manual/diagrams/render.sh" \
    "($("$plantuml" -version 2>/dev/null | head -n 1))" >&2
  exit 1
fi
echo "All diagrams are up to date."
