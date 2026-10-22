#!/usr/bin/env bash
# Regenerates the README screenshots.
#
# Serves the built interface, appends a setup script that fills it with
# plausible traffic through the same stand-in-bridge seam the tests use, and
# captures each view with headless Chrome at a size worth looking at.
#
#   npm run build:ui && tools/screenshots.sh
set -euo pipefail

cd "$(dirname "$0")/.."
BUILD=compiled/build
OUT=docs/screenshots
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
PORT=8766

test -f "$BUILD/index.html" || { echo "run npm run build:ui first"; exit 1; }

cp tools/screenshot-setup.js "$BUILD/screenshot-setup.js"
sed 's#</head>#<script src="screenshot-setup.js"></script></head>#' \
  "$BUILD/index.html" > "$BUILD/shot.html"

python3 -m http.server "$PORT" --directory "$BUILD" >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null || true; rm -f "$BUILD/shot.html" "$BUILD/screenshot-setup.js"' EXIT
sleep 1

mkdir -p "$OUT"
for shot in main load-test flow; do
  "$CHROME" --headless --disable-gpu --hide-scrollbars \
    --window-size=1600,1000 --force-device-scale-factor=2 \
    --virtual-time-budget=6000 \
    --screenshot="$OUT/$shot.png" \
    "http://127.0.0.1:$PORT/shot.html?shot=${shot%%-*}" >/dev/null 2>&1
  echo "$OUT/$shot.png"
done
