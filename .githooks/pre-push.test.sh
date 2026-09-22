#!/usr/bin/env bash
# Regression suite for .githooks/pre-push — the one gate in this project that is mechanism
# rather than prose, and therefore the one that has to be provable.
#
#   ./.githooks/pre-push.test.sh              # test the sibling pre-push
#   ./.githooks/pre-push.test.sh /path/to/hook
#
# It builds a throwaway bare remote and clone under $TMPDIR (removed on exit), points
# core.hooksPath at the hook under test, and then pushes things that MUST be refused and
# things that MUST be allowed. Both directions matter: a gate that blocks legitimate work
# gets bypassed, and a bypassed gate is worse than no gate.
#
# It uses STAND-IN leak patterns (LEAKNAME, leak-user), never the real private ones, so this
# file is safe to keep in a public repo.
#
# Two cases here are regressions with real history behind them, do not delete them:
#   - "leak added then removed mid-range": the endpoint-diff scan this replaced waved that
#     push through, which is how a personal name reached a test fixture on 2026-08-06.
#   - "editing the licence guard itself does not trip it": LICENCE_BODY is a list of licence
#     phrases, so before the pathspec exclusion the gate refused every edit to itself.
set -u
LAB=$(mktemp -d "${TMPDIR:-/tmp}/nexus-prepush-test.XXXXXX")
HOOK="${1:-$(cd "$(dirname "$0")" && pwd)/pre-push}"
trap 'rm -rf "$LAB"' EXIT

# Test patterns file: stand-ins for the real private ones, so nothing private is written here.
PATS="$LAB/patterns"
printf '%s\n' 'LEAKNAME' 'leak[-_ ]?user' > "$PATS"
chmod 600 "$PATS"
export NEXUS_LEAK_PATTERNS="$PATS"

git init -q --bare "$LAB/remote.git"
git clone -q "$LAB/remote.git" "$LAB/work" 2>/dev/null
W="$LAB/work"
git -C "$W" config user.name  "KD9TAW"
git -C "$W" config user.email "kd9taw@protonmail.com"
mkdir -p "$W/.githooks"; cp "$HOOK" "$W/.githooks/pre-push"; chmod +x "$W/.githooks/pre-push"
git -C "$W" config core.hooksPath "$W/.githooks"

PASS=0; FAIL=0
# expect <want:PASS|BLOCK> <name> -- <push args...>   (env prefix via ENVV)
expect() {
  want="$1"; name="$2"; shift 2
  out=$(cd "$W" && env ${ENVV:-} git push "$@" 2>&1); rc=$?
  got=PASS; [ $rc -ne 0 ] && got=BLOCK
  if [ "$got" = "$want" ]; then
    PASS=$((PASS+1)); printf '  ok   %-52s (%s)\n' "$name" "$got"
  else
    FAIL=$((FAIL+1)); printf '  FAIL %-52s want=%s got=%s\n' "$name" "$want" "$got"
    echo "$out" | sed 's/^/       | /'
  fi
  unset ENVV
}
note() { printf '\n== %s\n' "$*"; }

cd "$W"
# --- baseline: a clean branch, first push -------------------------------------
note "clean pushes"
echo "hello" > README.md; git add README.md
git commit -qm "init: readme"
expect PASS "clean first push of a new branch" -u origin HEAD:refs/heads/main

echo "more" >> README.md; git commit -qam "docs: more"
expect PASS "clean incremental push" origin HEAD:refs/heads/main

# --- 4a: leak added then REMOVED inside one range (the 2026-08-06 class) -------
note "content scrub"
git checkout -qb leaky
printf 'fixture LEAKNAME here\n' > tests/fixture.txt 2>/dev/null || { mkdir -p tests; printf 'fixture LEAKNAME here\n' > tests/fixture.txt; }
git add tests/fixture.txt; git commit -qm "test: add fixture"
printf 'fixture scrubbed\n' > tests/fixture.txt; git commit -qam "test: scrub fixture"
expect BLOCK "leak added then removed mid-range (new ref)" origin leaky:refs/heads/leaky
git checkout -q main; git branch -qD leaky

git checkout -qb leaky2
printf 'x LEAKNAME x\n' > f2.txt; git add f2.txt; git commit -qm "add f2"
git rm -q f2.txt; git commit -qm "drop f2"
expect BLOCK "leak added then removed mid-range (existing ref)" origin leaky2:refs/heads/main
git checkout -q main; git branch -qD leaky2

# --- 4b: leak surviving to the tip -------------------------------------------
git checkout -qb tipleak
printf 'contact leak-user\n' > t.txt; git add t.txt; git commit -qm "add t"
expect BLOCK "leak present in the pushed tree" origin tipleak:refs/heads/main
git checkout -q main; git branch -qD tipleak

# --- 4a: leak in a commit MESSAGE only ----------------------------------------
git checkout -qb msgleak
echo ok > m.txt; git add m.txt; git commit -qm "chore: reported by LEAKNAME"
expect BLOCK "leak in a commit message" origin msgleak:refs/heads/main
git checkout -q main; git branch -qD msgleak

# --- 4a: leak in commit METADATA (author name) --------------------------------
git checkout -qb metaleak
echo ok > n.txt; git add n.txt
GIT_AUTHOR_NAME="LEAKNAME" git commit -qm "chore: n"
expect BLOCK "leak in commit author NAME (identity kept)" origin metaleak:refs/heads/main
git checkout -q main; git branch -qD metaleak

# --- 4b: a SCRUB commit (pattern only on removed lines) must NOT be blocked ------
note "scrub commits must not be blocked (the 2026-08-05 class)"
# Put the leak on main via an unhooked push, so it is already-published history.
git checkout -q main
printf '! Copyright (C) 2026 LEAKNAME, KD9TAW\n' > hdr.f90; git add hdr.f90
git commit -qm "vendor: header"
git push -q --no-verify origin main:refs/heads/main
# Now the remediation commit: it only REMOVES the string.
printf '! Copyright (C) 2026 KD9TAW\n' > hdr.f90; git commit -qam "scrub: drop personal name from header"
expect PASS "scrub commit (pattern only on removed lines)" origin HEAD:refs/heads/main
# ...but re-adding it must still block.
printf '! Copyright (C) 2026 LEAKNAME, KD9TAW\n' > hdr.f90; git commit -qam "oops: re-add"
expect BLOCK "  ... re-adding the same string still blocks" origin HEAD:refs/heads/main
git reset -q --hard HEAD~1

# --- 3: identity ---------------------------------------------------------------
note "identity"
git checkout -qb wrongid
echo ok > w.txt; git add w.txt
GIT_AUTHOR_EMAIL="someone@example.com" git commit -qm "chore: w"
expect BLOCK "non-project author email" origin wrongid:refs/heads/main
git checkout -q main; git branch -qD wrongid

# The allowlist (added 2026-08-08). Default deny is kept: the case above must STAY blocked,
# which is why it is left immediately before these. An allowlist that admits everything and
# an allowlist that admits nothing both look "green" without a pair of tests pointing
# opposite ways, so every case below has its opposite.
ALLOW="$LAB/contributors"
printf '%s\n' '# a real outside contributor' 'contrib@example.org' > "$ALLOW"

git checkout -qb goodcontrib
echo ok > c.txt; git add c.txt
GIT_AUTHOR_EMAIL="contrib@example.org" GIT_AUTHOR_NAME="A Contributor" git commit -qm "fix: c"
# Unlisted FIRST, then listed. Order matters and this is a real trap: a blocked push sends
# nothing, but a PASSED one puts the commit on the remote, and the new-ref path only scans
# commits not already on SOME ref of that remote — so running these the other way round left
# the second push with an empty range and it was waved through, reporting a false PASS.
ENVV="NEXUS_CONTRIBUTORS=$LAB/nonexistent" \
  expect BLOCK "unlisted contributor refuses" origin goodcontrib:refs/heads/contribno
# ... and the SAME commit is accepted once the name is listed. Without this pair, a hook that
# admitted everything and one that admitted nothing would each pass one case and look right.
ENVV="NEXUS_CONTRIBUTORS=$ALLOW" \
  expect PASS "  ... same commit passes once allowlisted" origin goodcontrib:refs/heads/contribok
git checkout -q main; git branch -qD goodcontrib

# Role awareness (the split landed 2026-09-21; these tests did not, and the prefix went out
# untested on a gate whose own contributors file calls it "LOAD-BEARING"). A plain entry admits
# an identity as author AND committer; `committer:<email>` admits it as COMMITTER ONLY. The
# distinction is the whole point: GitHub's web-merge stamp legitimately appears as the committer
# on this repo's merge commits and must never be able to sign an AUTHOR line. Pooled into one
# list — which is what this check did before — admitting a committer silently admitted an author.
#
# Three cases, because two would not be enough. The PASS alone would also pass on a hook that
# ignored committers entirely, so the third case is what proves the committer list is consulted
# at all. Unlisted/BLOCK cases run before the PASS for the same reason the pair above does: a
# passed push lands the commit on the remote and empties the next range.
ROLES="$LAB/contributors-roles"
printf '%s\n' 'contrib@example.org' 'committer:bot@example.org' > "$ROLES"

git checkout -qb botauthor
echo ok > b1.txt; git add b1.txt
GIT_AUTHOR_EMAIL="bot@example.org" GIT_AUTHOR_NAME="A Bot" \
  GIT_COMMITTER_EMAIL="contrib@example.org" GIT_COMMITTER_NAME="A Contributor" \
  git commit -qm "chore: authored by the machine identity"
ENVV="NEXUS_CONTRIBUTORS=$ROLES" \
  expect BLOCK "a committer: entry does NOT admit authorship" origin botauthor:refs/heads/roleno
git checkout -q main; git branch -qD botauthor

# The committer list really is consulted — without this, the PASS below proves nothing.
git checkout -qb badcommitter
echo ok > b3.txt; git add b3.txt
GIT_AUTHOR_EMAIL="contrib@example.org" GIT_AUTHOR_NAME="A Contributor" \
  GIT_COMMITTER_EMAIL="stranger@example.org" GIT_COMMITTER_NAME="A Stranger" \
  git commit -qm "chore: committed by nobody listed"
ENVV="NEXUS_CONTRIBUTORS=$ROLES" \
  expect BLOCK "  ... and an unlisted COMMITTER still refuses" origin badcommitter:refs/heads/rolebad
git checkout -q main; git branch -qD badcommitter

# ... but the SAME machine identity passes in the one role it is listed for.
git checkout -qb botcommitter
echo ok > b2.txt; git add b2.txt
GIT_AUTHOR_EMAIL="contrib@example.org" GIT_AUTHOR_NAME="A Contributor" \
  GIT_COMMITTER_EMAIL="bot@example.org" GIT_COMMITTER_NAME="A Bot" \
  git commit -qm "chore: committed by the machine identity"
ENVV="NEXUS_CONTRIBUTORS=$ROLES" \
  expect PASS "  ... but DOES admit it as a committer" origin botcommitter:refs/heads/roleok
git checkout -q main; git branch -qD botcommitter

# THE ONE THAT MATTERS: the maintainer's other identity is denied BEFORE the allowlist is
# consulted, so listing it cannot readmit it. If this ever reports PASS, the deny/allow
# ordering in check 3 has been reversed and the gate no longer does its job.
printf '%s\n' 'work-identity@example.com' >> "$ALLOW"
git checkout -qb privid
echo ok > p.txt; git add p.txt
GIT_AUTHOR_EMAIL="work-identity@example.com" git commit -qm "chore: p"
ENVV="NEXUS_CONTRIBUTORS=$ALLOW NEXUS_PRIVATE_IDENT=work-identity@example.com" \
  expect BLOCK "maintainer's own identity, even when allowlisted" origin privid:refs/heads/privtest
git checkout -q main; git branch -qD privid

# --- 5: licence / NOTICE guard -------------------------------------------------
note "licence guard"
git checkout -qb vend
mkdir -p libtempo/vendor/thing; echo "int f(void){return 0;}" > libtempo/vendor/thing/a.c
git add libtempo/vendor/thing/a.c; git commit -qm "vendor: add thing"
expect BLOCK "new file under vendor/ with no NOTICE edit" origin vend:refs/heads/main
ENVV="NEXUS_ALLOW_VENDOR=1" expect PASS "  ... same push with NEXUS_ALLOW_VENDOR=1" origin vend:refs/heads/vendok
echo "thing — BSD-3-Clause (c) upstream" >> NOTICE; git add NOTICE; git commit -qm "NOTICE: thing"
expect PASS "  ... same push once NOTICE is touched" origin vend:refs/heads/main
git checkout -q main; git merge -q vend; git branch -qD vend

git checkout -qb licbody
mkdir -p crates/x; printf '/*\n * Redistribution and use in source and binary forms, with or\n * without modification, are permitted.\n */\nint g(void);\n' > crates/x/b.h
git add crates/x/b.h; git commit -qm "feat: add b.h"
expect BLOCK "licence BODY text added outside vendor/, no NOTICE" origin licbody:refs/heads/main
git checkout -q main; git branch -qD licbody

git checkout -qb selfedit
# The guard's own marker list is licence phrases; editing the gate must not trip the gate.
printf '\n# touched by a maintainer\n' >> .githooks/pre-push
git add .githooks/pre-push; git commit -qm "hooks: edit the gate itself"
expect PASS "editing the licence guard itself does not trip it" origin selfedit:refs/heads/main
git checkout -q main; git merge -q selfedit; git branch -qD selfedit

git checkout -qb ownfile
mkdir -p crates/y; printf '//! Our own module header.\n//! Copyright 2026 KD9TAW.\npub fn z() {}\n' > crates/y/c.rs
git add crates/y/c.rs; git commit -qm "feat: own module"
expect PASS "ordinary new source file (no licence body) passes" origin ownfile:refs/heads/main
git checkout -q main; git merge -q ownfile; git branch -qD ownfile

# --- 2: release-tag gate --------------------------------------------------------
note "release-tag gate"
git tag v9.9.9
expect BLOCK "pushing refs/tags/v* " origin v9.9.9
ENVV="NEXUS_RELEASE_APPROVED=1" expect PASS "  ... with NEXUS_RELEASE_APPROVED=1" origin v9.9.9
git tag nightly-1
expect PASS "pushing a non-v tag is not gated" origin nightly-1

# --- 2b: green-CI gate on the release tag -------------------------------------
# Approval says the release SHOULD go out; this says the commit it is cut from was
# actually proven. Both must hold, and the cases below are here because the FIRST run of
# this suite against the new check passed for the wrong reason: the lab clone has no
# scripts/ at all, so the "gate script absent" branch fired and nothing was exercised.
# A gate that is only ever skipped is indistinguishable from one that works.
#
# NEXUS_REQUIRE_GREEN_CI points the hook at a stub instead of the real script, so these
# stay offline and deterministic. The real predicate has its own suite
# (scripts/require-green-ci.test.sh) and was run against the live API in both directions;
# what is under test HERE is only that the hook calls it, honours its exit status, and
# refuses the push when it says no.
note "release-tag green-CI gate"
printf '#!/usr/bin/env bash\nexit 0\n' > "$LAB/ci-green"; chmod +x "$LAB/ci-green"
printf '#!/usr/bin/env bash\necho "stub: no green ci run" >&2\nexit 1\n' > "$LAB/ci-red"; chmod +x "$LAB/ci-red"
printf '#!/usr/bin/env bash\necho "stub: cannot read" >&2\nexit 2\n' > "$LAB/ci-unreadable"; chmod +x "$LAB/ci-unreadable"

git tag v9.9.10
ENVV="NEXUS_RELEASE_APPROVED=1 NEXUS_REQUIRE_GREEN_CI=$LAB/ci-green" \
  expect PASS "approved + green CI" origin v9.9.10
git tag v9.9.11
ENVV="NEXUS_RELEASE_APPROVED=1 NEXUS_REQUIRE_GREEN_CI=$LAB/ci-red" \
  expect BLOCK "approved but NO green CI run" origin v9.9.11
git tag v9.9.12
# Fail-closed: an unreadable answer (no gh, no network, API error) is not a green one.
ENVV="NEXUS_RELEASE_APPROVED=1 NEXUS_REQUIRE_GREEN_CI=$LAB/ci-unreadable" \
  expect BLOCK "approved, CI evidence UNREADABLE (exit 2)" origin v9.9.12
git tag v9.9.13
# Order matters: no approval must refuse before the CI check is even consulted, so a green
# commit can never imply approval.
ENVV="NEXUS_REQUIRE_GREEN_CI=$LAB/ci-green" \
  expect BLOCK "green CI does not substitute for approval" origin v9.9.13
git tag nightly-2
ENVV="NEXUS_RELEASE_APPROVED=1 NEXUS_REQUIRE_GREEN_CI=$LAB/ci-red" \
  expect PASS "a non-v tag is not CI-gated either" origin nightly-2

# --- 1: wrong remote -------------------------------------------------------------
note "wrong-remote guard"
git remote add tempo "https://github.com/kd9taw/tempo.git"
expect BLOCK "push to kd9taw/tempo" tempo HEAD:refs/heads/main

# --- misc: deletion, no-op, patterns file missing ---------------------------------
note "edge cases"
expect PASS "branch deletion" origin --delete vendok
expect PASS "no-op push (nothing new)" origin HEAD:refs/heads/main
ENVV="NEXUS_LEAK_PATTERNS=$LAB/nope" expect BLOCK "missing patterns file refuses (fail-closed)" origin HEAD:refs/heads/main
printf 'LEAKNAME\n\nleak[-_ ]?user\n' > "$LAB/blankpat"; chmod 600 "$LAB/blankpat"
ENVV="NEXUS_LEAK_PATTERNS=$LAB/blankpat" expect BLOCK "blank line in patterns file is refused, not obeyed" origin HEAD:refs/heads/main

# --- 7: CHANGELOG merge-superset --------------------------------------------------
# A merge may not delete a CHANGELOG bullet a parent had. Both directions, plus the
# override, because a gate proven in one direction is half a test. The lab clone has no
# scripts/ of its own, so the tool is copied in; without it check 7 is inert by design.
#
# THE FIXTURE HEADINGS ARE LONG ON PURPOSE. The first version of this test used headings
# like "- **Main bullet.**" (11 chars) and the BLOCK case PASSED: the tool ignores any
# heading under 12 characters, because a shorter one cannot identify an entry. The test was
# wrong, not the tool — but only the must-trip direction could have revealed that, which is
# exactly why it is here. Keep these headings sentence-shaped, like real entries.
note "CHANGELOG merge-superset"
CLTOOL=$(cd "$(dirname "$HOOK")/.." && pwd)/scripts/check-changelog-merges.mjs
if [ ! -f "$CLTOOL" ] || ! command -v node >/dev/null 2>&1; then
  printf '  skip  check 7 (tool or node not available)\n'
else
  mkdir -p "$W/scripts"; cp "$CLTOOL" "$W/scripts/check-changelog-merges.mjs"
  git -C "$W" checkout -q main 2>/dev/null || git -C "$W" checkout -q -B main
  printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- **The base entry that was already here.** Body text here.\n' > "$W/CHANGELOG.md"
  git -C "$W" add CHANGELOG.md scripts/check-changelog-merges.mjs
  git -C "$W" commit -qm "changelog: base"
  git -C "$W" push -q origin HEAD:refs/heads/main 2>/dev/null

  # main gains a bullet; a branch gains a different one.
  git -C "$W" checkout -q -b cl-side
  printf -- '- **The branch adds a way to see the band.** Added on the branch.\n' >> "$W/CHANGELOG.md"
  git -C "$W" commit -qm "changelog: side bullet" -- CHANGELOG.md
  git -C "$W" checkout -q main
  printf -- '- **Mainline adds a way to log a contact.** Added on main.\n' >> "$W/CHANGELOG.md"
  git -C "$W" commit -qm "changelog: main bullet" -- CHANGELOG.md
  git -C "$W" push -q origin HEAD:refs/heads/main 2>/dev/null

  # A GOOD merge: keep both bullets.
  git -C "$W" checkout -q -b cl-good
  git -C "$W" merge -q --no-commit --no-ff cl-side >/dev/null 2>&1 || true
  printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- **The base entry that was already here.** Body text here.\n- **Mainline adds a way to log a contact.** Added on main.\n- **The branch adds a way to see the band.** Added on the branch.\n' > "$W/CHANGELOG.md"
  git -C "$W" add CHANGELOG.md
  git -C "$W" commit -qm "Merge cl-side (kept both)"
  expect PASS "a merge that keeps both parents' bullets" origin cl-good:refs/heads/cl-good

  # A BAD merge: the resolution drops main's bullet — the exact 1.13.0 defect.
  git -C "$W" checkout -q main
  git -C "$W" checkout -q -b cl-bad
  git -C "$W" merge -q --no-commit --no-ff cl-side >/dev/null 2>&1 || true
  printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- **The base entry that was already here.** Body text here.\n- **The branch adds a way to see the band.** Added on the branch.\n' > "$W/CHANGELOG.md"
  git -C "$W" add CHANGELOG.md
  git -C "$W" commit -qm "Merge cl-side (DROPPED main's bullet)"
  expect BLOCK "a merge that deletes a parent's bullet" origin cl-bad:refs/heads/cl-bad
  ENVV="NEXUS_ALLOW_CHANGELOG_LOSS=1" \
    expect PASS "  ... with NEXUS_ALLOW_CHANGELOG_LOSS=1" origin cl-bad:refs/heads/cl-bad
  ENVV="NEXUS_SKIP_CHANGELOG_MERGE=1" \
    expect PASS "  ... with NEXUS_SKIP_CHANGELOG_MERGE=1" origin cl-bad:refs/heads/cl-bad
fi


printf '\n== %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
