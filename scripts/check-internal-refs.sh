#!/usr/bin/env bash
# scripts/check-internal-refs.sh — fail if the tree points at private working material.
#
# WHY. This repository is public and its working notes are not: the maintainer's planning
# documents, design notes and agent reports live outside it. A comment, a test name or a doc that
# cites one of them, by its file name, by a section number or by the id of the task, the agent,
# the batch or the decision that produced it (a ruling's number, an agent's or a task's id, a
# batch number, "design §" or "plan §" and a section, a file under tasks/), tells a reader
# nothing and publishes the shape of the private tree. Say what the thing IS instead: "the
# operator's rule that a program's own audio goes out only after it asks a voice mode", "the
# earlier measurement".
#
# WHAT IT READS. Every tracked file, not a diff, so a line added anywhere is caught by the next
# run. The CHANGELOG's `## [Unreleased]` section only: released sections and
# docs/RELEASE_NOTES-* are history, published as written. Binary files are skipped.
#
# CONTROLS FIRST, FAIL CLOSED. Before the tree is read, the same scan runs over a scratch
# repository planted with every kind of reference (each must be found) and with the look-alikes
# below (none may be), so a pattern that has stopped matching cannot pass as a clean tree. The
# planted references are made up and assembled at run time, so this file carries none itself.
#
# THE FALSE POSITIVES IT ALLOWS are in ALLOW, each a path, a pattern the line must match and the
# reason. Look-alikes the patterns already pass: frequencies (7.074 MHz), a real standard's
# section (RFC 6455 §4.2.2, TCI 2.0 §4.2, §97.119), a Rust error code (E0433), a callsign whose
# digit is single (RA6LH, N0CALL), a grid square in a longer word (EN50), and English ("a batch
# of 8", "the plan", "a design choice", "off-plan").
#
# Usage: scripts/check-internal-refs.sh [--root DIR]   (default: this checkout)
# Exit: 0 clean · 1 a reference was found (each is printed) · 2 the controls failed or the scan
# could not run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ "${1:-}" = "--root" ]; then ROOT="$(cd "${2:?--root needs a directory}" && pwd)"; fi

# Task, agent, batch and ruling numbers.
REFS_RE='\b(N[0-9]{2,3}[a-z]?|[Bb]atch ?[0-9]{2,3}|R[AX][0-9]{2,3})\b|\b[Rr]uling [0-9]{3}\b'
# Pointers into a private document: its sections, its numbered questions, rules and test rows,
# its file name, a note under tasks/, a memory's name.
DOCS_RE="\\b(design|plan)('s)?,? §[0-9]|\\bplan (piece )?[A-Z][0-9]+\\b|\\bdesign Q[0-9]+\\b"
DOCS_RE+="|\\bS-M[0-9]+\\b|\\b[Tt]ask [A-Z][0-9]+(\\.[0-9]+)?\\b"
DOCS_RE+="|[Ss]pec (TX[- ]safety|invariant|TX invariant) #?[0-9]"
DOCS_RE+="|\\b[a-z-]+/tasks\\b|\\btasks/[A-Za-z0-9_.-]+\\.md\\b|\\b(rx|ra|rw|n)[0-9]{2,3}-[a-z0-9-]+\\.md\\b"
DOCS_RE+="|\\b[A-Z]{2,}(-[A-Z0-9]+)*-(PLAN|SPEC|DESIGN)(-20[0-9]{2}-[0-9]{2}-[0-9]{2}|\\.md|,? §)"
DOCS_RE+="|\\b(feedback|reference|project)-[a-z0-9]+(-[a-z0-9]+){2,}\\b|\\[\\[[a-z0-9]+(-[a-z0-9]+)+\\]\\]"

# path <TAB> pattern the hit's line must match <TAB> why it is allowed
ALLOW=$(cat <<'EOF'
crates/tempo-core/tests/fixtures/wsjtx-callsign-oracle.json	R[A]99	callsign shapes in the WSJT-X callsign oracle (RA is a Russian prefix)
crates/js8/src/proto/grid.rs	"R[A]90"	a Maidenhead grid square in a test
ui/src/i18n/hardcoded-strings.test.ts	[Bb]atch (1[0-9]|2[0-2])\b	the string migration's own numbered steps, each defined in this file's log
crates/tempo-core/src/fd_rules.seed.json	Batch 1[0] installs a country file	pending: a seed edit needs a new stamp and republishes the rules file; reword it at the next re-stamp
crates/tempo-audio/src/service.rs	spec invariant 1[2]	pending: in the decode loop, which other work is changing; reword it when that lands
EOF
)

# Prints every hit under $1 as path:line:text, the allowed ones removed. Exit 2 when git fails.
scan() {
  local root=$1 out rc
  set +e
  out=$(git -C "$root" grep -n -I -E -e "$REFS_RE" -e "$DOCS_RE" -- . \
    ':!CHANGELOG.md' ':!docs/RELEASE_NOTES-*' ':!scripts/check-internal-refs.sh')
  rc=$?
  set -e
  [ "$rc" -le 1 ] || { echo "check-internal-refs: git grep failed ($rc) in $root" >&2; return 2; }
  if [ -f "$root/CHANGELOG.md" ]; then
    out+=$'\n'$(awk '/^## \[/{u=($0 ~ /^## \[Unreleased\]/)} u{print "CHANGELOG.md:" NR ":" $0}' \
      "$root/CHANGELOG.md" | grep -a -E -e "$REFS_RE" -e "$DOCS_RE" || true)
  fi
  local hit path line text p re why ok
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    path=${hit%%:*}; line=${hit#*:}; text=${line#*:}
    ok=0
    while IFS=$'\t' read -r p re why; do
      if [ "$p" = "$path" ] && printf '%s\n' "$text" | grep -q -E -e "$re"; then ok=1; break; fi
    done <<<"$ALLOW"
    [ "$ok" = 1 ] || printf '%s\n' "$hit"
  done <<<"$out"
}

# The controls. Made-up references, one of each kind, each of which must be found ($z keeps
# every one of them out of this file's own text)…
z=0
HIT=("// the operator ruling 9$z$z holds it" "// found by RX$z$z" "// RA$z$z$z measured it"
  "// N$z$z${z}b fixed it" "// batch$z$z$z landed it" "// Batch $z$z adds the tiles"
  "// (design §$z.1)" "// the example design's §$z.2" "// (plan §$z.3 item 4)" "// (plan Z$z)"
  "// design Q$z" "// S-M$z: the burst" "// (Task Z$z.1)" "// spec invariant $z" "// spec TX safety #$z"
  "// see tasks/notes-$z.md" "// in example/tasks" "// n$z$z-example-report.md"
  "// EXAMPLE-DESIGN-20$z$z-01-01" "// EXAMPLE-SPEC §$z" "// see feedback-an-example-$z"
  "// [[an-example-$z]]" "// Some(\"RA9$z\") in a file its allowance does not name")
# …and look-alikes, none of which may be.
PASS=('// 7.074 MHz' '// RFC 6455 §4.2.2' '// TCI 2.0 §4.2' '// §97.119' '// error E0433' '// RA6LH$2'
  '// N0CALL' '// grid EN50' '// a batch of 8 frames' '// the batch size' '// the plan'
  '// a design choice' '// an OFF-PLAN dial' '// the lead-in' '// tasks/ is gitignored'
  '// a feedback loop' '// a project-wide rule' '// reference level' '// spec §5')

# Runs the scan over a scratch repository holding the controls, an allowed line and its
# neighbour that is not, and a CHANGELOG with a released section; the hits must be exactly the
# planted references, the neighbour and the unreleased line.
controls() (
  d=$(mktemp -d "${TMPDIR:-/tmp}/internal-refs.XXXXXX")
  trap 'rm -rf "$d"' EXIT
  git -C "$d" init -q
  mkdir -p "$d/docs" "$d/crates/js8/src/proto"
  printf '%s\n' "${HIT[@]}" >"$d/hit.rs"
  printf '%s\n' "${PASS[@]}" >"$d/pass.rs"
  printf '%s\n' "            Some(\"RA9$z\")," "            Some(\"RA9$((z + 1))\")," >"$d/crates/js8/src/proto/grid.rs"
  printf '%s\n' '# Changelog' '## [Unreleased]' "- per ruling 9$z$z" '## [1.0.0]' "- old: batch 7$z" >"$d/CHANGELOG.md"
  printf '%s\n' "batch 8$z" >"$d/docs/RELEASE_NOTES-1.0.0.md"
  git -C "$d" add -- hit.rs pass.rs crates/js8/src/proto/grid.rs CHANGELOG.md docs/RELEASE_NOTES-1.0.0.md
  hits=$(scan "$d") || exit 2
  want=$( (seq 1 "${#HIT[@]}" | sed 's/^/hit.rs:/'; echo 'crates/js8/src/proto/grid.rs:2'; echo 'CHANGELOG.md:3') | sort)
  got=$(printf '%s\n' "$hits" | cut -d: -f1,2 | sort)
  if [ "$got" != "$want" ]; then
    echo "check-internal-refs: CONTROLS FAILED — the scan misses what it must find, or finds what it must pass." >&2
    diff <(printf '%s\n' "$want") <(printf '%s\n' "$got") | sed 's/^/  /' >&2 || true
    exit 2
  fi
)

controls || exit 2
found=$(scan "$ROOT") || exit 2
if [ -n "$found" ]; then
  echo "Internal references in the tree — say what each thing IS instead (see this script's header):"
  printf '%s\n' "$found" | cut -c1-200 | sed 's/^/  /'
  exit 1
fi
echo "check-internal-refs: clean. Controls: $((${#HIT[@]} + 2)) planted references found; $((${#PASS[@]} + 3)) look-alikes, allowed and released lines passed."
