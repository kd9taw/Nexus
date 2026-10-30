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
# conclusions, a run from a branch whose link is not the shipped one, and the override. Each is fed to the SAME script through its own
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

green_run() { printf '[{"databaseId":1,"headBranch":"main","status":"completed","conclusion":"success","headSha":"%s","event":"push","createdAt":"2026-09-15T00:00:00Z","url":"u"}]' "$SHA"; }

echo "require-green-ci regression suite — $SCRIPT"
echo
echo "1. THE POSITIVE CONTROL. Everything below is only meaningful because this passes."
run 0 "completed + success + right sha + push  => PASS" "$(green_run)"
run 0 "same, via workflow_dispatch             => PASS" \
  "[{\"databaseId\":1,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "2. NOT COMPLETED. The clause 'not failed' gets wrong: conclusion is null while a run"
echo "   is still going, and null != \"failure\"."
for st in in_progress queued requested waiting pending; do
  run 1 "status=$st, conclusion=null          => REFUSE" \
    "[{\"databaseId\":2,\"headBranch\":\"main\",\"status\":\"$st\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
done

echo
echo "3. COMPLETED BUT NOT SUCCESS. Every one of these is != \"failure\"; none is evidence."
echo "   'cancelled' is the 1.12.0 specimen and 43% of recent main runs."
for c in cancelled timed_out action_required neutral skipped stale failure; do
  run 1 "conclusion=$c                  => REFUSE" \
    "[{\"databaseId\":3,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"$c\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
done

echo
echo "4. WRONG COMMIT. A green run on a DIFFERENT sha — this is the case an ancestry test"
echo "   ('is the green commit an ancestor of the tag?') would wave through, and it is the"
echo "   exact footing 1.12.0 shipped on: green parents, an unproven tag."
run 1 "green run, but headSha is another commit => REFUSE" \
  "[{\"databaseId\":4,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$OTHER\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "5. PULL_REQUEST RUNS. headSha is the branch commit, but Actions tested the MERGE of"
echo "   it with the base tip — a tree that is not what the tag points at."
run 1 "green, right sha, event=pull_request    => REFUSE" \
  "[{\"databaseId\":5,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"pull_request\",\"createdAt\":\"t\",\"url\":\"u\"}]"

echo
echo "6. NO RUNS AT ALL — the release.yml hole as it stands today."
run 1 "empty run list                          => REFUSE" '[]'

echo
echo "7. MIXED LISTS. One good run among noise must still pass; noise alone must not."
run 0 "cancelled + in_progress + one green      => PASS" \
  "[{\"databaseId\":6,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"cancelled\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":7,\"headBranch\":\"main\",\"status\":\"in_progress\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":8,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"
run 1 "cancelled + in_progress, no green        => REFUSE" \
  "[{\"databaseId\":6,\"headBranch\":\"main\",\"status\":\"completed\",\"conclusion\":\"cancelled\",\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":7,\"headBranch\":\"main\",\"status\":\"in_progress\",\"conclusion\":null,\"headSha\":\"$SHA\",\"event\":\"push\",\"createdAt\":\"t\",\"url\":\"u\"}]"

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
printf '[{"databaseId":9,"headBranch":"main","status":"completed","conclusion":"cancelled","headSha":"%s","event":"push","createdAt":"t","url":"u"}]' "$SHA" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
for want in 'conclusion=cancelled' 'NEXUS_ALLOW_UNVERIFIED_CI' "$SHA"; do
  if printf '%s' "$msg" | grep -qF "$want"; then
    pass=$((pass+1)); printf '  ok   %-58s\n' "refusal names '$want'"
  else
    fail=$((fail+1)); printf '  FAIL %-58s\n' "refusal does NOT name '$want'"
  fi
done

echo
echo "12. WHERE THE RUN CAME FROM. ci.yml links the Windows and Pi builds with the shipped fat-LTO"
echo "    profile only on main, release/* and a tag; a green run from any other branch proves the"
echo "    tests but not the link that ships, so it is not release evidence."
branch_run() { printf '[{"databaseId":12,"headBranch":%s,"status":"completed","conclusion":"success","headSha":"%s","event":"workflow_dispatch","createdAt":"t","url":"u"}]' "$1" "$SHA"; }
run 0 "green on release/1.2.3-2026-10-26        => PASS" "$(branch_run '"release/1.2.3-2026-10-26"')"
run 0 "green on the tag being released ($REF)    => PASS" "$(branch_run "\"$REF\"")"
run 0 "green on main                            => PASS" "$(branch_run '"main"')"
run 1 "green on land/some-batch                 => REFUSE" "$(branch_run '"land/some-batch"')"
run 1 "green on a topic branch                  => REFUSE" "$(branch_run '"fix/something"')"
run 1 "green on a DIFFERENT tag (v1.0.0)        => REFUSE" "$(branch_run '"v1.0.0"')"
run 1 "green on 'mainline' (not main)           => REFUSE" "$(branch_run '"mainline"')"
run 1 "green on 'release' (no slash)            => REFUSE" "$(branch_run '"release"')"
run 1 "green on 'xrelease/1.2' (prefix inside)  => REFUSE" "$(branch_run '"xrelease/1.2"')"
run 1 "green, headBranch null                   => REFUSE" "$(branch_run 'null')"
run 1 "green, headBranch missing                => REFUSE" \
  "[{\"databaseId\":13,\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"}]"
run 0 "land run green + release/* run green     => PASS" \
  "[{\"databaseId\":14,\"headBranch\":\"land/x\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":15,\"headBranch\":\"release/9.9\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"}]"
printf '[{"databaseId":16,"headBranch":"land/x","status":"completed","conclusion":"success","headSha":"%s","event":"workflow_dispatch","createdAt":"t","url":"u"}]' "$SHA" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
if printf '%s' "$msg" | grep -qF 'branch=land/x'; then
  pass=$((pass+1)); printf '  ok   %-58s\n' "a land-branch refusal names the branch"
else
  fail=$((fail+1)); printf '  FAIL %-58s\n' "a land-branch refusal does NOT name the branch"
fi

echo
echo "13. A BRANCH NAMED LIKE THE TAG. The run list records a name, not whether it was a branch or"
echo "    the tag; a run accepted only because its name equals the tag is refused when a branch of"
echo "    that name exists."
RUNENV="NEXUS_CI_REF_IS_BRANCH=no"  run 0 "run on '$REF', no branch of that name       => PASS" "$(branch_run "\"$REF\"")"
RUNENV="NEXUS_CI_REF_IS_BRANCH=yes" run 1 "run on '$REF', a branch '$REF' exists       => REFUSE" "$(branch_run "\"$REF\"")"
RUNENV="NEXUS_CI_REF_IS_BRANCH=yes" run 0 "same, but a release/* run also green       => PASS" \
  "[{\"databaseId\":17,\"headBranch\":\"$REF\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"},
    {\"databaseId\":18,\"headBranch\":\"release/9.9.9\",\"status\":\"completed\",\"conclusion\":\"success\",\"headSha\":\"$SHA\",\"event\":\"workflow_dispatch\",\"createdAt\":\"t\",\"url\":\"u\"}]"
RUNENV="NEXUS_CI_REF_IS_BRANCH=maybe" run 2 "branch answer neither yes nor no           => ERROR" "$(branch_run "\"$REF\"")"
RUNENV="NEXUS_CI_REF_IS_BRANCH=yes" run 0 "a branch '$REF' exists, run is on main     => PASS" "$(branch_run '"main"')"
RUNENV=""
branch_run "\"$REF\"" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" NEXUS_CI_REF_IS_BRANCH=yes "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
if printf '%s' "$msg" | grep -qF "a BRANCH called '$REF' exists"; then
  pass=$((pass+1)); printf '  ok   %-58s\n' "the refusal says a branch has the tag's name"
else
  fail=$((fail+1)); printf '  FAIL %-58s\n' "the refusal does NOT say a branch has the tag's name"
fi

echo
echo "14. THE ADVICE WORKS WHERE THE REFUSAL IS MET. At the pre-push hook the tag is not on the"
echo "    remote yet, so the remedy runs CI on main or a release/* branch, never on the tag."
printf '[{"databaseId":19,"headBranch":"land/x","status":"completed","conclusion":"success","headSha":"%s","event":"workflow_dispatch","createdAt":"t","url":"u"}]' "$SHA" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
for want in "--ref main" "git push origin $SHA:refs/heads/release/9.9.9" "--ref release/9.9.9"; do
  if printf '%s' "$msg" | grep -qF -- "$want"; then
    pass=$((pass+1)); printf '  ok   %-58s\n' "advice names '$want'"
  else
    fail=$((fail+1)); printf '  FAIL %-58s\n' "advice does NOT name '$want'"
  fi
done
if printf '%s' "$msg" | grep -qF -- "--ref $REF"; then
  fail=$((fail+1)); printf '  FAIL %-58s\n' "advice still says to run CI on the unpushed tag"
else
  pass=$((pass+1)); printf '  ok   %-58s\n' "advice never runs CI on the unpushed tag"
fi

echo
echo "15. THE LIVE BRANCH LOOKUP, through a stand-in gh on PATH (no NEXUS_CI_RUNS_JSON): only gh's"
echo "    'Branch not found' 404 means no branch; a 404 for the repository, a network error or a"
echo "    rate limit is an unreadable answer (exit 2); runs on main or release/* never ask."
mkdir -p "$LAB/bin"
cat > "$LAB/bin/gh" <<'FAKE'
#!/usr/bin/env bash
# Stand-in for gh: `run list` answers from $FAKE_RUNS; `api .../branches/<name>` answers by
# $FAKE_BRANCH and records the path it was asked for in $FAKE_LOG.
case "$1 $2" in
  "run list") cat "$FAKE_RUNS"; exit 0 ;;
  "api "*)
    echo "$2" >> "$FAKE_LOG"
    case "$FAKE_BRANCH" in
      200)       echo "v9.9.9"; exit 0 ;;
      nobranch)  echo "gh: Branch not found (HTTP 404)" >&2; exit 1 ;;
      norepo)    echo "gh: Not Found (HTTP 404)" >&2; exit 1 ;;
      ratelimit) echo "gh: API rate limit exceeded (HTTP 403)" >&2; exit 1 ;;
      network)   echo "error connecting to api.github.com" >&2; exit 1 ;;
    esac ;;
esac
echo "fake gh: unexpected call: $*" >&2; exit 99
FAKE
chmod +x "$LAB/bin/gh"
live() {  # live <expected-exit> <name> <FAKE_BRANCH> <headBranch> [ref]
  local want="$1" name="$2" mode="$3" hb="$4" ref="${5:-$REF}" out rc
  printf '[{"databaseId":20,"headBranch":"%s","status":"completed","conclusion":"success","headSha":"%s","event":"workflow_dispatch","createdAt":"t","url":"u"}]' "$hb" "$SHA" > "$LAB/live.json"
  : > "$LAB/api.log"
  out=$(env -u NEXUS_CI_RUNS_JSON PATH="$LAB/bin:$PATH" FAKE_RUNS="$LAB/live.json" FAKE_BRANCH="$mode" FAKE_LOG="$LAB/api.log" \
        "$SCRIPT" --sha "$SHA" --ref "$ref" --repo owner/repo 2>&1)
  rc=$?
  if [ "$rc" = "$want" ]; then
    pass=$((pass+1)); printf '  ok   %-58s exit %s\n' "$name" "$rc"
  else
    fail=$((fail+1)); printf '  FAIL %-58s exit %s (wanted %s)\n' "$name" "$rc" "$want"
    printf '%s\n' "$out" | sed 's/^/       | /'
  fi
}
live 1 "tag run, branch lookup 200 (a branch exists) => REFUSE" 200 "$REF"
live 0 "tag run, 'Branch not found' 404            => PASS"   nobranch "$REF"
live 2 "tag run, repository 'Not Found' 404        => ERROR"  norepo "$REF"
live 2 "tag run, rate limited (403)                => ERROR"  ratelimit "$REF"
live 2 "tag run, network error                     => ERROR"  network "$REF"
live 0 "main run never asks about a branch         => PASS"   network main
if [ -s "$LAB/api.log" ]; then
  fail=$((fail+1)); printf '  FAIL %-58s\n' "a main run asked the branch API"
else
  pass=$((pass+1)); printf '  ok   %-58s\n' "a main run asked the branch API nothing"
fi
live 0 "tag 'v9.9.9#x' is looked up encoded       => PASS"   nobranch 'v9.9.9#x' 'v9.9.9#x'
if grep -qxF 'repos/owner/repo/branches/v9.9.9%23x' "$LAB/api.log"; then
  pass=$((pass+1)); printf '  ok   %-58s\n' "the lookup path encodes '#'"
else
  fail=$((fail+1)); printf '  FAIL %-58s\n' "the lookup path was: $(cat "$LAB/api.log")"
fi
printf '[{"databaseId":21,"headBranch":"land/x","status":"completed","conclusion":"success","headSha":"%s","event":"workflow_dispatch","createdAt":"t","url":"u"}]' "$SHA" > "$LAB/runs.json"
msg=$(NEXUS_CI_RUNS_JSON="$LAB/runs.json" GITHUB_ACTIONS=true GITHUB_RUN_ID=4242 "$SCRIPT" --sha "$SHA" --ref "$REF" --repo owner/repo 2>&1)
if printf '%s' "$msg" | grep -qF 'gh run rerun 4242 --failed'; then
  pass=$((pass+1)); printf '  ok   %-58s\n' "in a workflow, the advice re-runs the failed jobs"
else
  fail=$((fail+1)); printf '  FAIL %-58s\n' "in a workflow, the advice does NOT re-run the failed jobs"
fi

echo
echo "--------------------------------------------------------------"
printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
echo "ALL GREEN"
