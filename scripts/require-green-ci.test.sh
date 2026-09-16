#!/usr/bin/env bash
# Regression suite for scripts/require-green-ci — the gate that keeps a release from
# publishing on CI evidence that does not exist.
#
#   ./scripts/require-green-ci.test.sh              # test the sibling script
#   ./scripts/require-green-ci.test.sh /path/to/script
#
# WHAT IT DOES AND DOES NOT COVER, said plainly. The primary evidence for this gate is that
# it was run against the LIVE GitHub API in both directions: it refuses kd9taw/nexus
# 5badb66d (v1.12.0's tag commit, whose only ci.yml run is CANCELLED) and it passes
# c200a027 (a real completed/success run). Those two cannot live in a test suite — they
# need the network and the answers move as history moves.
#
# What IS here is every shape the live API will not produce on demand: a run still in
# progress, a headSha that is not the commit, a pull_request run, the five non-success
# conclusions, and the override. Each is fed to the SAME script through its own
# NEXUS_CI_RUNS_JSON seam — the file under test is the file that ships, not a copy.
#
# BOTH DIRECTIONS EVERY TIME. A refusal case is worthless without a pass case built the
# same way: if the script refused unconditionally every refusal test would still be green,
# and the gate would block every release. Each block below therefore has its positive
# control right next to it, and the suite fails if the control does not pass.
set -u

SCRIPT="${1:-$(cd "$(dirname "$0")" && pwd)/require-green-ci}"
LAB=$(mktemp -d "${TMPDIR:-/tmp}/nexus-relgate-test.XXXXXX")
trap 'rm -rf "$LAB"' EXIT

SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
OTHER=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
REF=v9.9.9

pass=0; fail=0

# run <expected-exit> <name> <runs-json>   [extra env assignments via RUNENV]
run() {
  local want="$1" name="$2" json="$3" out rc
  printf '%s' "$json" > "$LAB/runs.json"
  out=$(env NEXUS_CI_RUNS_JSON="$LAB/runs.json" ${RUNENV:-} \
        "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
  rc=$?
  if [ "$rc" = "$want" ]; then
    pass=$((pass+1)); printf '  ok   %-58s exit %s\n' "$name" "$rc"
  else
    fail=$((fail+1)); printf '  FAIL %-58s exit %s (wanted %s)\n' "$name" "$rc" "$want"
    printf '%s\n' "$out" | sed 's/^/       | /'
  fi
}

green_run() { printf '[{"databaseId":1,"status":"completed","conclusion":"success","headSha":"%s","event":"push","createdAt":"2026-09-15T00:00:00Z","url":"u"}]' "$SHA"; }

echo "require-green-ci regression suite — $SCRIPT"
echo
echo "1. THE POSITIVE CONTROL. Everything below is only meaningful because this passes."
run 0 "completed + success + right sha + push  => PASS" "$(green_run)"
run 0 "same, via workflow_dispatch             => PASS" \
  "[{\"databaseId\":1,\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "2. NOT COMPLETED. The clause 'not failed' gets wrong: conclusion is null while a run"
echo "   is still going, and null != \"failure\"."
for st in in_progress queued requested waiting pending; do
  run 1 "status=$st, conclusion=null          => REFUSE" \
    "[{\"databaseId\":2,\"status\":\"$st\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
done

echo
echo "3. COMPLETED BUT NOT SUCCESS. Every one of these is != \"failure\"; none is evidence."
echo "   'cancelled' is the 1.12.0 specimen and 43% of recent main runs."
for c in cancelled timed_out action_required neutral skipped stale failure; do
  run 1 "conclusion=$c                  => REFUSE" \
    "[{\"databaseId\":3,\"status\":\"completed\",\"conclusion\":\"$c\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
done

echo
echo "4. WRONG COMMIT. A green run on a DIFFERENT sha — this is the case an ancestry test"
echo "   ('is the green commit an ancestor of the tag?') would wave through, and it is the"
echo "   exact footing 1.12.0 shipped on: green parents, an unproven tag."
run 1 "green run, but headSha is another commit => REFUSE" \
  "[{\"databaseId\":4,\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$OTHER\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "5. PULL_REQUEST RUNS. headSha is the branch commit, but Actions tested the MERGE of"
echo "   it with the base tip — a tree that is not what the tag points at."
run 1 "green, right sha, event=pull_request    => REFUSE" \
  "[{\"databaseId\":5,\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"pull_request\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "6. NO RUNS AT ALL — the release.yml hole as it stands today."
run 1 "empty run list                          => REFUSE" '[]'

echo
echo "7. MIXED LISTS. One good run among noise must still pass; noise alone must not."
run 0 "cancelled + in_progress + one green      => PASS" \
  "[{\"databaseId\":6,\"status\":\"completed\",\"conclusion\":\"cancelled\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":7,\"status\":\"in_progress\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":8,\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
run 1 "cancelled + in_progress, no green        => REFUSE" \
  "[{\"databaseId\":6,\"status\":\"completed\",\"conclusion\":\"cancelled\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":7,\"status\":\"in_progress\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "8. THE OVERRIDE. Keyed to the ref, deliberately not a boolean, so one left set after"
echo "   an incident cannot wave the NEXT release through."
RUNENV="NEXUS_ALLOW_UNVERIFIED_CI=$REF" run 0 "override = this ref, no runs at all       => PASS" '[]'
RUNENV="NEXUS_ALLOW_UNVERIFIED_CI=1"    run 1 "override = 1 (a boolean does nothing)     => REFUSE" '[]'
RUNENV="NEXUS_ALLOW_UNVERIFIED_CI=v1.2.3" run 1 "override = a DIFFERENT, stale ref        => REFUSE" '[]'
RUNENV="NEXUS_ALLOW_UNVERIFIED_CI="     run 1 "override empty                           => REFUSE" '[]'
RUNENV=""

echo
echo "9. UNREADABLE EVIDENCE IS NOT GREEN EVIDENCE (exit 2, never 0)."
run 2 "response is an object, not an array      => ERROR" '{"message":"Not Found"}'
run 2 "response is a bare string                => ERROR" '"rate limit exceeded"'

echo
echo "10. USAGE. A short SHA must be a usage error, not a refusal: the API returns [] for"
echo "    one, which reads exactly like 'this commit has no CI run'."
short_out=$("$SCRIPT" --sha 5badb66d --ref "$REF" --repo owner/repo 2>&1); short_rc=$?
if [ "$short_rc" = "2" ]; then
  pass=$((pass+1)); printf '  ok   %-58s exit 2\n' "short sha => usage error"
else
  fail=$((fail+1)); printf '  FAIL %-58s exit %s (wanted 2)\n' "short sha => usage error" "$short_rc"
  printf '%s\n' "$short_out" | sed 's/^/       | /'
fi
for args in "--sha $SHA" "--ref $REF" "--sha $SHA --ref $REF --bogus x"; do
  # shellcheck disable=SC2086
  "$SCRIPT" $args --repo owner/repo >/dev/null 2>&1; rc=$?
  if [ "$rc" = "2" ]; then
    pass=$((pass+1)); printf '  ok   %-58s exit 2\n' "'$args' => usage error"
  else
    fail=$((fail+1)); printf '  FAIL %-58s exit %s (wanted 2)\n' "'$args' => usage error" "$rc"
  fi
done

echo
echo "11. THE REFUSAL HAS TO SAY WHY. A bare exit 1 cannot be told apart from a broken gate,"
echo "    which is how a gate gets worked around instead of fixed."
printf '[{"databaseId":9,"status":"completed","conclusion":"cancelled","headSha":"%s","event":"push","createdAt":"t","url":"u"}]' "$SHA" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
for want in 'conclusion=cancelled' 'NEXUS_ALLOW_UNVERIFIED_CI' "$SHA"; do
  if printf '%s' "$msg" | grep -qF "$want"; then
    pass=$((pass+1)); printf '  ok   %-58s\n' "refusal names '$want'"
  else
    fail=$((fail+1)); printf '  FAIL %-58s\n' "refusal does NOT name '$want'"
  fi
done

echo
echo "--------------------------------------------------------------"
printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
echo "ALL GREEN"
