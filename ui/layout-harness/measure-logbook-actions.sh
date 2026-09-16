#!/usr/bin/env bash
# Measure the Logbook row-action cluster against its grid track in a REAL layout engine.
#
# ⚠️ jsdom lays out nothing — `getBoundingClientRect` is 0×0 there — so "does the button
# cluster fit the track floor" cannot be answered from vitest. This runs the harness page
# under headless Chrome, which does lay out, and prints what `logbook-actions.html`
# measured. The vitest guard (ui/src/styles-logbook-actions.test.tsx) enforces the budget
# these numbers established; it does not re-derive them.
#
# It is a MEASUREMENT, not a CI gate: no browser is installed on the CI images, and a gate
# that silently no-ops when Chrome is missing is worse than no gate. It refuses loudly.
#
# Usage:
#   ./measure-logbook-actions.sh                  # the working tree's sheet
#   ./measure-logbook-actions.sh /abs/other.css   # any other sheet (before/after evidence)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

chrome=""
for c in google-chrome google-chrome-stable chromium chromium-browser; do
  if command -v "$c" >/dev/null 2>&1; then chrome="$c"; break; fi
done
if [ -z "$chrome" ]; then
  echo "no Chrome/Chromium on PATH — this measurement cannot be taken here." >&2
  echo "Install one, or open logbook-actions.html in a browser and call run()." >&2
  exit 2
fi

# With no argument the page links ../src/styles.css itself, so there is no copy to keep in
# step and no way to measure a stale sheet by accident. An argument points it at another one
# (the before/after evidence: git show origin/main:ui/src/styles.css > /tmp/main.css).
sheet="${1:-}"
if [ -z "$sheet" ]; then
  url="file://$here/logbook-actions.html"
else
  [ -r "$sheet" ] || { echo "cannot read sheet: $sheet" >&2; exit 2; }
  url="file://$here/logbook-actions.html?sheet=file://$(cd "$(dirname "$sheet")" && pwd)/$(basename "$sheet")"
fi

profile="$(mktemp -d)"
trap 'rm -rf "$profile"' EXIT
dom=$("$chrome" --headless=new --disable-gpu --no-sandbox --allow-file-access-from-files \
  --user-data-dir="$profile" --virtual-time-budget=6000 \
  --dump-dom "$url" 2>/dev/null)
status=$?
if [ $status -ne 0 ]; then
  echo "headless Chrome exited $status" >&2
  exit 1
fi

# The page writes its measurements into the output <pre>. An empty one means the script did
# not run — say so rather than printing nothing and looking successful.
#
# Done in awk, not as a `sed` line range: the payload is multi-line when the measurement
# succeeds but SINGLE-line when the page reports an error, and a sed range never closes on its
# own start line — so the error case ran to EOF and printed the whole page source after the
# error. This stops at the first closing tag wherever it falls.
json=$(printf '%s' "$dom" | awk -v open='<pre id="out">' -v endtag='</pre>' '
  !on { i = index($0, open); if (!i) next; $0 = substr($0, i + length(open)); on = 1 }
  { j = index($0, endtag); if (j) { print substr($0, 1, j - 1); exit } print }
')
if [ -z "$json" ]; then
  echo "the harness page produced no measurements — did logbook-actions.html throw?" >&2
  exit 1
fi
json=$(printf '%s' "$json" | sed 's/&quot;/"/g; s/&lt;/</g; s/&gt;/>/g; s/&amp;/\&/g')
printf '%s\n' "$json"
# The page reports a stylesheet that did not load, or a throw inside run(), as an `error` key
# rather than as silence. Carry that out in the EXIT STATUS too: a measurement tool that
# prints its own failure and exits 0 is the shape that gets read as a green.
case "$json" in
  *'"error"'*) exit 1 ;;
esac
