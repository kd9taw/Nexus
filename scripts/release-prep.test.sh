#!/usr/bin/env bash
# Regression suite for scripts/release-prep's release-notes refusal.
#
#   ./scripts/release-prep.test.sh              # test the sibling script
#   ./scripts/release-prep.test.sh /path/to/script
#
# THE BUG IT HOLDS DOWN. The refusal for a missing or commit-list docs/RELEASE_NOTES-<ver>.md
# used to run AFTER the manifests were bumped and the CHANGELOG stamped. A refusal left the tree
# half-bumped, the re-run then refused as "not greater than current", and the TLE seed and the
# alignment check never ran: 1.17.0's seed had to be re-cut by hand. So a refusal must change
# NOTHING, and the re-run after writing the notes must go through.
#
# Hermetic: each case builds a throwaway repo under $TMPDIR holding only the files the script
# touches, and runs the real script in it. No network (the scratch repo has no gen-tles.mjs, so
# the seed step takes its own best-effort warning path) and no `gh`. Needs git, node and npm.
#
# BOTH DIRECTIONS. A refusal case is worthless without a pass case built the same way: a script
# that refused unconditionally would pass every refusal case, so the passes sit beside them and
# the suite fails if they do not pass.
set -u

SCRIPT="${1:-$(cd "$(dirname "$0")" && pwd)/release-prep}"
LAB=$(mktemp -d "${TMPDIR:-/tmp}/nexus-relprep-test.XXXXXX")
trap 'rm -rf "$LAB"' EXIT

pass=0; fail=0
ok()  { echo "ok   - $1"; pass=$((pass+1)); }
bad() { echo "FAIL - $1"; fail=$((fail+1)); }

# repo <dir>: a committed tree at 9.8.0 with an [Unreleased] CHANGELOG section.
repo() {
  local d="$1"
  mkdir -p "$d/src-tauri" "$d/ui" "$d/docs/guide"
  printf '[package]\nname = "nexus"\nversion = "9.8.0"\n' > "$d/src-tauri/Cargo.toml"
  printf '{\n  "productName": "Nexus",\n  "version": "9.8.0"\n}\n' > "$d/src-tauri/tauri.conf.json"
  printf '{\n  "name": "nexus-ui",\n  "version": "9.8.0",\n  "private": true\n}\n' > "$d/ui/package.json"
  printf 'cff-version: 1.2.0\nversion: 9.8.0\n' > "$d/CITATION.cff"
  printf '# Guide\n\n**This guide documents Nexus 9.8.0.**\n' > "$d/docs/guide/index.md"
  printf '# Changelog\n\n## [Unreleased]\n\n- Something an operator will notice.\n\n## [9.8.0] — 2026-01-01\n' > "$d/CHANGELOG.md"
  git -C "$d" init -q
  git -C "$d" -c user.name=test -c user.email=test@example.invalid add src-tauri ui docs CITATION.cff CHANGELOG.md
  git -C "$d" -c user.name=test -c user.email=test@example.invalid commit -qm fixture
}

prose() { printf 'Nexus 9.9.0 makes the thing an operator will notice work.\n' > "$1/docs/RELEASE_NOTES-9.9.0.md"; }
commit_list() { printf -- '- fix: one\n- feat: two\n- chore: three\n' > "$1/docs/RELEASE_NOTES-9.9.0.md"; }

# prep <dir> [env assignments...]: run the real script inside <dir>; prints its exit code.
prep() {
  local d="$1"; shift
  (cd "$d" && env "$@" bash "$SCRIPT" 9.9.0 >"$d.out" 2>&1); echo $?
}

version_of() { grep -om1 '"version": *"[^"]*"' "$1/src-tauri/tauri.conf.json" | grep -o '[0-9][^"]*'; }

# untouched <dir> <untracked-allowed>: nothing tracked changed, and nothing new but the notes.
untouched() {
  local d="$1" st
  st=$(git -C "$d" status --porcelain --untracked-files=all | grep -v ' docs/RELEASE_NOTES-9.9.0.md$')
  [ -z "$st" ] && [ "$(version_of "$d")" = 9.8.0 ] && grep -q '^## \[Unreleased\]' "$d/CHANGELOG.md"
}

# 1. Notes missing: refused, and NOTHING changed.
d="$LAB/missing"; repo "$d"
rc=$(prep "$d")
[ "$rc" = 1 ] && grep -q 'RELEASE_NOTES-9.9.0.md is missing' "$d.out" \
  && ok "missing notes: refused (exit $rc)" || bad "missing notes: want exit 1 with the missing-notes error, got $rc"
untouched "$d" && ok "missing notes: the tree is untouched" \
  || bad "missing notes: the refusal changed the tree ($(git -C "$d" status --porcelain | tr '\n' ' '))"

# 2. Then the notes are written and the SAME command is re-run: it goes through, every step runs.
prose "$d"
rc=$(prep "$d")
[ "$rc" = 0 ] && ok "re-run after writing the notes: exit 0" || bad "re-run after writing the notes: want 0, got $rc ($(tail -1 "$d.out"))"
grep -q 'TLE seed' "$d.out" && ok "re-run: the seed step ran" || bad "re-run: the seed step never ran"
grep -q '^aligned: Cargo.toml=9.9.0 tauri.conf.json=9.9.0 package.json=9.9.0' "$d.out" \
  && ok "re-run: the alignment check ran and passed" || bad "re-run: no alignment line"
grep -q '^## \[9.9.0\] — ' "$d/CHANGELOG.md" && ! grep -q '^## \[Unreleased\]' "$d/CHANGELOG.md" \
  && ok "re-run: the CHANGELOG heading is stamped once" || bad "re-run: the CHANGELOG heading is wrong"

# 3. Notes that read as a commit list: refused, and NOTHING changed.
d="$LAB/commitlist"; repo "$d"; commit_list "$d"
rc=$(prep "$d")
[ "$rc" = 1 ] && grep -q 'reads as an auto-generated commit list' "$d.out" \
  && ok "commit-list notes: refused (exit $rc)" || bad "commit-list notes: want exit 1 with the commit-list error, got $rc"
untouched "$d" && ok "commit-list notes: the tree is untouched" \
  || bad "commit-list notes: the refusal changed the tree ($(git -C "$d" status --porcelain | tr '\n' ' '))"

# 4. Positive control: prose notes present from the start go straight through.
d="$LAB/prose"; repo "$d"; prose "$d"
rc=$(prep "$d")
[ "$rc" = 0 ] && [ "$(version_of "$d")" = 9.9.0 ] && ok "prose notes: bumped to 9.9.0 (exit 0)" \
  || bad "prose notes: want exit 0 at 9.9.0, got $rc at $(version_of "$d")"

# 5. Positive control: the deliberate override still continues without notes.
d="$LAB/override"; repo "$d"
rc=$(prep "$d" NEXUS_ALLOW_NO_RELEASE_NOTES=1)
[ "$rc" = 0 ] && [ "$(version_of "$d")" = 9.9.0 ] && grep -q 'continuing without' "$d.out" \
  && ok "override: continues without notes (exit 0)" || bad "override: want exit 0 at 9.9.0, got $rc at $(version_of "$d")"

echo "# $pass passed, $fail failed"
[ "$fail" = 0 ] && [ "$pass" -gt 0 ]
