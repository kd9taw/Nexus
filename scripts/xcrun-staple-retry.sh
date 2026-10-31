#!/usr/bin/env bash
# xcrun-staple-retry.sh — the `xcrun` the macOS release bundler runs, so stapling the
# notarization ticket onto the .app is retried and its failure is seen.
#
# release.yml links this file as `xcrun` into a directory it puts first on PATH for
# `cargo tauri bundle`. Every call except `xcrun stapler staple` goes straight to the real
# xcrun (notarytool included), unchanged.
#
# WHY. tauri-cli 2.11.5 (tauri-macos-sign 2.3.4, `staple_app`) runs `xcrun stapler staple -v`
# right after Apple accepts the notarization and throws away its output AND its exit status.
# Apple can take a while to publish a freshly accepted ticket where the stapler looks for it,
# so a staple attempted too soon fails, the bundler carries on, and the DMG and the updater
# tarball are built from an .app with no ticket in it. The Intel leg of a release dry run did
# exactly that: notarization accepted, spctl "Notarized Developer ID" half a minute later, and
# `stapler validate` "does not have a ticket stapled to it". An unstapled app still opens on a
# Mac that is online; offline, Gatekeeper cannot find the ticket and refuses it.
#
# So: staple, then validate, up to NEXUS_STAPLE_TRIES times (default 10), NEXUS_STAPLE_WAIT
# seconds apart (default 30), and give up after that, never waiting without bound. Because the
# bundler swallows everything this prints, each line also goes to NEXUS_STAPLE_LOG, which the
# workflow prints and checks for a "stapled" line once the bundler returns.
set -uo pipefail

real=${NEXUS_REAL_XCRUN:-/usr/bin/xcrun}

if [ "${1-}" != stapler ] || [ "${2-}" != staple ]; then
  exec "$real" "$@"
fi

target=${!#}
tries=${NEXUS_STAPLE_TRIES:-10}
wait=${NEXUS_STAPLE_WAIT:-30}

say() {
  echo "$1" >&2
  if [ -n "${NEXUS_STAPLE_LOG:-}" ]; then echo "$1" >> "$NEXUS_STAPLE_LOG"; fi
}

for ((i = 1; i <= tries; i++)); do
  if out=$("$real" "$@" 2>&1) && out=$("$real" stapler validate "$target" 2>&1); then
    say "stapled $target (attempt $i of $tries)"
    exit 0
  fi
  # The stapler's last lines name why (a ticket not found yet, a network error); the verbose
  # request dump above them is left out.
  say "staple attempt $i of $tries failed for $target: $(printf '%s\n' "$out" | tail -n 3 | tr '\n' ' ')"
  if [ "$i" -lt "$tries" ]; then
    say "the ticket may not be published yet; trying again in ${wait}s"
    sleep "$wait"
  fi
done

say "no ticket stapled to $target after $tries attempts"
exit 65
