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

printf '\n== %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
