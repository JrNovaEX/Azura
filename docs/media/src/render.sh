#!/bin/sh
# Renders every poster in docs/media/src to docs/media/<name>.png (1280x640)
# with a headless Chromium browser. Run from docs/media.
#
# Set BROWSER to point at whichever one you have, for example:
#   BROWSER=/usr/bin/chromium ./src/render.sh
set -eu

cd "$(dirname "$0")"

if [ -z "${BROWSER:-}" ]; then
  for candidate in chromium chromium-browser google-chrome google-chrome-stable msedge; do
    if command -v "$candidate" >/dev/null 2>&1; then
      BROWSER="$(command -v "$candidate")"
      break
    fi
  done
fi

if [ -z "${BROWSER:-}" ]; then
  echo "no browser found; set BROWSER to one of:" >&2
  echo "  chromium, chromium-browser, google-chrome, msedge" >&2
  exit 1
fi

echo "using $BROWSER"

for page in src/[0-9]*.html; do
  [ -e "$page" ] || continue
  name="$(basename "$page" .html)"
  "$BROWSER" \
    --headless=new \
    --disable-gpu \
    --hide-scrollbars \
    --force-device-scale-factor=1 \
    --no-sandbox \
    --window-size=1280,640 \
    --screenshot="$(pwd)/$name.png" \
    "file://$(pwd)/$page" >/dev/null 2>&1
  echo "rendered $name.png"
done