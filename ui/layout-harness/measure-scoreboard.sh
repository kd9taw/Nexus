#!/usr/bin/env bash
# Measure the contest scoreboard in a REAL layout engine and print the JSON.
#
# ⚠️ jsdom lays out nothing — `getBoundingClientRect` is 0×0 there, so whether the
# scoreboard row still wraps rather than scrolls once the rate meter's tiles join it
# cannot be checked from vitest. This runs the harness page under headless
# Chrome, which does lay out, and prints what `scoreboard.html`'s `run()` measured.
#
# It is a MEASUREMENT, not a CI gate: no browser is installed on the CI images, and a
# gate that silently no-ops when Chrome is missing is worse than no gate (it reports
# green having checked nothing). It refuses loudly instead, and the numbers it prints
# are what a batch quotes.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

chrome=""
for c in google-chrome google-chrome-stable chromium chromium-browser; do
  if command -v "$c" >/dev/null 2>&1; then chrome="$c"; break; fi
done
if [ -z "$chrome" ]; then
  echo "no Chrome/Chromium on PATH — this measurement cannot be taken here." >&2
  echo "Install one, or open scoreboard.html in a browser and call run()." >&2
  exit 2
fi

# The stylesheet must be the REAL one (README step 1) or the numbers describe a sheet
# nobody ships. Refuse rather than measure a stale copy.
if ! diff -q "$here/styles.css" "$here/../src/styles.css" >/dev/null; then
  echo "layout-harness/styles.css is stale — run: cp ../src/styles.css ." >&2
  exit 2
fi

profile="$(mktemp -d)"
trap 'rm -rf "$profile"' EXIT
dom=$("$chrome" --headless=new --disable-gpu --no-sandbox \
  --user-data-dir="$profile" --virtual-time-budget=4000 \
  --dump-dom "file://$here/scoreboard.html" 2>/dev/null)
status=$?
if [ $status -ne 0 ]; then
  echo "headless Chrome exited $status" >&2
  exit 1
fi

# The page writes its measurements into <pre id="out">. An empty one means the script
# did not run — say so rather than printing nothing and looking successful.
# The JSON is multi-line, so this is a RANGE extraction, not a line substitution.
json=$(printf '%s' "$dom" | sed -n '/<pre id="out">/,/<\/pre>/p' \
  | sed 's/.*<pre id="out">//; s/<\/pre>.*//')
if [ -z "$json" ]; then
  echo "the harness page produced no measurements — did scoreboard.html throw?" >&2
  exit 1
fi
printf '%s\n' "$json" | sed 's/&quot;/"/g; s/&lt;/</g; s/&gt;/>/g; s/&amp;/\&/g'
