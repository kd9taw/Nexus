#!/usr/bin/env bash
# stamp-version.sh — put the RELEASE TAG's full version into tauri.conf.json before a build.
#
# Run once, immediately after checkout, in EVERY release.yml job that builds or names an
# artifact. Miss one and that platform ships an artifact versioned differently from its
# siblings, which is worse than the bug this closes.
#
# ── WHY THIS EXISTS ────────────────────────────────────────────────────────────────────────
# v1.11.0-beta.2 shipped an app that reported 1.11.0. release.yml stripped the prerelease
# suffix and REQUIRED tauri.conf.json to equal the base version, so the build was stamped
# 1.11.0 and named Nexus_1.11.0_x64-setup.exe while latest.json advertised 1.11.0-beta.2.
#
# By SemVer a prerelease sorts BELOW its release — 1.11.0 > 1.11.0-beta.2 — so every beta
# tester's app believed it was NEWER than the feed. A later 1.11.0-beta.3 was never offered,
# and neither was the eventual stable 1.11.0: same number, different content, and the updater
# only ever offers a strictly newer version. One prerelease tag stranded its testers
# permanently, with the app cheerfully reporting they were current. That is exactly the
# failure CLAUDE.md's "tester builds never consume a public version number" rule exists to
# prevent; it had never bitten because this was the first prerelease through release.yml.
#
# ── THE SUFFIX BELONGS TO THE BUILD, NOT THE REPO ──────────────────────────────────────────
# main keeps the plain version (1.11.0) — scripts/release-prep sets that and it stays true —
# and the full version is derived HERE from the tag. Nobody commits a suffixed version, so
# there is no window in which main disagrees with itself and no second manifest to forget.
#
#   v1.11.0        -> conf stays 1.11.0          (byte-identical: the stable path is untouched)
#   v1.11.0-beta.3 -> conf becomes 1.11.0-beta.3
#   a branch ref   -> no-op (the workflow_dispatch dry-run builds main exactly as it stands)
#
# The tag's BASE must still equal the committed version, so "someone forgot to bump" is still
# caught — and now it is caught in the first seconds of every build job instead of forty
# minutes later in publish, after five platforms have been built for nothing.
#
# Everything downstream follows on its own, because every one of them reads tauri.conf.json:
# the four bundlers name their files from it (tauri-bundler `settings.version_string()`), the
# app's own version is it (tauri-codegen puts `config.version` in `PackageInfo`, which is what
# `app_version` and `check_for_update` report), and the manual's cover reads it directly.
#
#   usage: scripts/stamp-version.sh [ref] [tauri.conf.json path]
#
# Both default to CI's values ($GITHUB_REF_NAME and the path every other step greps). The
# second argument exists so the logic can be exercised against a scratch copy — the real
# script against a real file, never a re-typed version of it.
set -euo pipefail

ref="${1:-${GITHUB_REF_NAME:-}}"
conf_path="${2:-src-tauri/tauri.conf.json}"

# Read it the way every consumer reads it (release.yml's macos + manual jobs, publish.yml's
# ancestry gate, ci.yml's manual-epub), so this check and those consumers cannot disagree.
read_conf() { grep -m1 '"version"' "$1" | sed -E 's/.*"version": *"([^"]+)".*/\1/'; }

# Only a TAG stamps. A workflow_dispatch dry-run runs on a branch and must build the tree as it
# stands. GITHUB_REF_TYPE is set by Actions on both paths; unset means a local run, where the
# ref passed in is taken at face value.
if [ "${GITHUB_REF_TYPE:-tag}" != "tag" ]; then
  echo "stamp-version: ref '$ref' is a ${GITHUB_REF_TYPE} and not a tag — leaving $conf_path at $(read_conf "$conf_path")"
  exit 0
fi

[ -n "$ref" ] || { echo "stamp-version: no ref given and \$GITHUB_REF_NAME is empty" >&2; exit 1; }

# The same shape publish.yml validates. A tag that reaches this workflow but is not a version is
# worth stopping for: a silent no-op here would be the quiet wrong-version class this script
# exists to end.
printf '%s' "$ref" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$' \
  || { echo "stamp-version: tag '$ref' is not vX.Y.Z[-suffix] — refusing to stamp from it" >&2; exit 1; }

ver="${ref#v}"
base="${ver%%-*}"

conf=$(read_conf "$conf_path")
[ -n "$conf" ] || { echo "stamp-version: no \"version\" key found in $conf_path" >&2; exit 1; }

# The cross-check release.yml has always made, kept honest and moved earlier. Both directions of
# failure are named, because the message that says only "forgot to bump" sends the operator
# looking for the opposite mistake.
[ "$conf" = "$base" ] || {
  {
    echo "stamp-version: tag $ref has base $base, but $conf_path says $conf."
    echo "  The committed version must be the PLAIN base version ($base): the prerelease suffix"
    echo "  is stamped from the tag at build time and never committed. So either the version"
    echo "  bump did not land in this commit, or a suffixed version was committed by hand"
    echo "  (scripts/release-prep X.Y.Z-suffix is for hand-built tester installers, which have"
    echo "  no tag to stamp from)."
  } >&2
  exit 1
}

if [ "$ver" = "$base" ]; then
  echo "stamp-version: $ref is a stable tag and $conf_path already says $conf — nothing to write"
  exit 0
fi

# First occurrence only, and the line is rewritten rather than the file re-serialised: a JSON
# round-trip would reformat a file four other steps read with grep. awk because `sed -i` and the
# `0,/re/` address are GNU-only spellings and the macOS job runs BSD sed.
tmp=$(mktemp)
awk -v ver="$ver" '
  !stamped && /"version"[[:space:]]*:[[:space:]]*"[^"]*"/ {
    sub(/"version"[[:space:]]*:[[:space:]]*"[^"]*"/, "\"version\": \"" ver "\"")
    stamped = 1
  }
  { print }
' "$conf_path" > "$tmp"
cat "$tmp" > "$conf_path"   # preserve the file's mode/inode; the checkout owns it
rm -f "$tmp"

# Read it back through read_conf — the same expression the consumers use — so a substitution
# that landed somewhere else, or not at all, fails here rather than shipping.
got=$(read_conf "$conf_path")
[ "$got" = "$ver" ] || {
  echo "stamp-version: wrote $ver but $conf_path reads back '$got' — refusing to build" >&2
  exit 1
}
echo "stamp-version: $conf_path stamped $conf -> $ver for prerelease tag $ref"
