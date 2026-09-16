#!/usr/bin/env node
// scripts/check-changelog-merges.mjs — a merge may not LOSE a CHANGELOG bullet.
//
// WHY THIS EXISTS. On 2026-09-15 an audit of what Nexus shipped without telling anyone found that
// 28 finished, operator-facing CHANGELOG bullets had been written by the people who built the
// features, committed, and then DELETED by six merge resolutions during the 1.13.0 integration.
// None was ever restored. The Unreleased section held 42 bullets; 70 had been written. Forty per
// cent of the release notes were gone, and nothing anywhere went red:
//
//   e3b34846  Merge feat/1.13-batch2-logging      8 bullets lost
//   f3947003  Merge feat/1.13-batch3-operating    6
//   aa67f526  Merge feat/remote-audio-listen      6
//   4f7e17c1  Merge feat/1.13-remote-leftovers    5
//   1235bf0c  Merge feat/1.13-pota-parks          2
//   0a38477f  Merge feat/1.13-batch7-tx           1
//
// Each merge compiled, tested green and was reviewed. The loss was invisible because the ONLY
// automation that reads CHANGELOG.md runs at release time and is version-scoped: release-prep
// renames the [Unreleased] heading, and release-docs.mjs checks the heading exists and that any
// issue numbers CITED in the section are closed — and returns OK when the section cites none. An
// empty section is a green result. At push time nothing anywhere reads the file at all.
//
// Same defect class as the shared-file hunk leak: the resolution was verified by "it builds",
// never by "it kept what both sides contributed".
//
// WHAT THIS CHECKS. One invariant, and it is the true statement rather than a proxy for it:
//
//     A MERGE NEVER LOSES A CHANGELOG BULLET.
//
// Stated over the WHOLE FILE, not the [Unreleased] section — see WHY THE WHOLE FILE below.
//
// IF YOU ARE HERE TO SIMPLIFY THIS FILE, READ THIS FIRST.
//
//   The first version of this check was correct about the invariant and still unusable. It
//   reported every merge that had EVER dropped a bullet, so the moment the 28 lost entries were
//   restored it went on reporting all 28 — green work, permanently red gate. A gate that stays
//   red after it fires once is worse than no gate, because it trains everyone to push past it.
//
//   That was found NOT BY REASONING ABOUT THE DESIGN, BUT BY RUNNING THE TOOL AGAINST THE REAL
//   REPAIR AND READING THE EXIT CODE. The design argument was sound and the gate was still
//   unusable.
//
//   The same thing happened three more times in one session: the paragraph-based key looked
//   obviously right and produced ten false positives from one innocent body reword; the CI copy
//   of this check looked free and would have reported a FALSE GREEN on every shallow clone; and
//   the regression test's must-block case passed against an 11-character heading the tool
//   silently ignores. Every one was caught by running something, none by thinking harder.
//
//   So: whatever you change here, point it at real history and read the exit code. The person
//   who next simplifies this check will be reasoning, not running. Do not be that person.
//
// Deleting a bullet at a merge is never correct WITHIN A RELEASE CYCLE. That scoping is not a
// detail — see DEFAULT RANGE below; it is the difference between a gate that is true and one that
// cries wolf. A trip here is the defect, not a judgement call, which is why this can be binding
// where a "did you write an entry?" check could only ever be advisory: it cannot fire on a
// refactor, a test-only branch, or a doc change.
//
// DEFAULT RANGE IS THE CURRENT RELEASE CYCLE (last tag..HEAD), AND THAT IS MEASURED, NOT ASSUMED.
// Run over the whole of Nexus's history this check reports 65 findings, of which 37 are innocent.
// Every one of those 37 is a RELEASE-TIME editorial rewrite, and they are legitimate:
//
//   - 18f1f049 "Bridge to the published 0.4.1 history (SourceForge main)" — a `-s ours` ancestry
//     merge. 12 findings. Exempted below by tree comparison, not by SHA.
//   - e329cbb5 "Release notes: collapse six tester builds into the 0.21.5 story" — six bullets
//     deliberately collapsed into one narrative, reported at two later merges. 12 findings.
//   - a35826d5 "release: 1.4.0" rewrote "The waterfall can run the other way up" as "The
//     waterfall runs downward now, and you can put it back" (CHANGELOG.md:3759). 3 findings.
//   - A logbook-error bullet split into two (CHANGELOG.md:1045 and :1064). 2 findings.
//   - A Remote flicker bullet folded into a combined one (:389), and "Backup and Restore moved to
//     their own Config tab" reworded (:2174). 2 findings.
//
// All are heading rewrites made while cutting a release. Over the CURRENT cycle
// (v1.12.0..origin/main) the measured false-positive count is ZERO: 28 reported, 28 real,
// independently confirmed by a separate awk pass over the [Unreleased] section. So the honest
// claim is not "this check never false-positives" — it is "within a release cycle, on the only
// history we have, it did not". Run it wider and read the output as a report, not a gate.
//
// WHAT IT DOES NOT CATCH, written down rather than papered over:
//   - A bullet MOVED to the wrong ### subsection. Still in the file, so this is silent.
//   - A bullet whose HEADING is rewritten on one side, where neither heading is a prefix of the
//     other. Reported as a loss. This is the residual false positive; see above for its real rate.
//   - A DELIBERATE removal — a branch that reverts a feature and correctly deletes its bullet.
//     That is a CORRECT deletion, not a sloppy one, so no amount of normalising can tell it from
//     the defect: both look exactly like a bullet one parent has and the merge does not. It takes
//     the override, NEXUS_ALLOW_CHANGELOG_LOSS=1, which is named in the refusal so someone
//     reverting on purpose is unblocked in seconds while someone who clobbered a merge is stopped.
//     Deliberately not auto-detected from a "Revert ..." subject: a heuristic that waves a merge
//     through on the shape of a commit message is exactly how a real loss would get missed, and
//     the override leaves a trace in the shell history where a silent bypass leaves none.
//     Measured: NO revert has ever deleted a CHANGELOG bullet in this repo's history (224 merges
//     swept), so this is cheap insurance, not a frequent need — which is part of why the default
//     is to block rather than warn.
//   - A feature that never had a bullet written at all — 274 files across eight branches did
//     exactly that in the same release. Nothing here can know. That gap belongs to the
//     operator-review path lens, deliberately advisory, and is a separate tool.
//   - Content introduced BY a merge resolution is fine and ignored; the invariant is one-way.
//   - A bullet whose heading is under 12 characters is INVISIBLE to this check. Anything
//     shorter cannot identify an entry — two unrelated releases would collide on it — so it is
//     dropped rather than guessed at. Real entries are sentences ("Stop transmitting when SWR
//     is high"), so this costs nothing in practice, but it is a genuine hole and not a rounding
//     error: a merge could delete `- **Fixed.**` and nothing here would say so. Found by the
//     pre-push regression suite, whose first fixture used 11-character headings and silently
//     passed a merge that had really dropped one — which is the whole argument for building
//     the test from a merge that must trip rather than from one that must not.
//
// EXIT CODES — the vocabulary scripts/gates and scripts/operator-review already use:
//   0  every merge in range keeps every parent's bullets (or NEXUS_ALLOW_CHANGELOG_LOSS=1)
//   1  at least one merge lost a bullet
//   2  usage error, or a git/CHANGELOG read this tool cannot account for
//   3  passed, but coverage is incomplete (a merge was skipped and named)
//
// BLOCK, NOT WARN — and that is a measured decision, not a preference. Over the current release
// cycle the false-positive count is ZERO (28 reported, 28 real). Over the whole of history every
// innocent finding is a RELEASE-TIME heading rewrite, which the default range excludes, and no
// revert has ever deleted a bullet here. An advisory check everyone reads beats a blocking one
// that gets --no-verify'd — but that trade only applies when a check cries wolf, and on the range
// it actually runs, this one does not. If that ever stops being true, demote it rather than
// leaving people to route around it, and say so here.
//
// LOUD, NEVER SILENT. A merge whose CHANGELOG this tool cannot read is NAMED and downgrades the
// run to exit 3, never silently treated as clean. A checker that swallows what it does not
// understand manufactures exactly the confidence it exists to replace.
//
// *** READ-ONLY. Writes no file, stages nothing; every git call is a query. Other agents work in
// *** this checkout — see CLAUDE.md's working protocol.
//
// Run:  node scripts/check-changelog-merges.mjs                 (last tag..HEAD — THE GATE)
//       node scripts/check-changelog-merges.mjs <range>         (e.g. v1.12.0..origin/main)
//       node scripts/check-changelog-merges.mjs --selftest      (both directions, on real history)
//
//       node scripts/check-changelog-merges.mjs --all
//         *** A REPORT, NOT A GATE. --all is not the gate run wider; it is a different thing. ***
//         Over the whole of this repo's history it prints 19 findings, and ALL NINETEEN ARE
//         LEGITIMATE — release-time heading rewrites, every one traced by hand (the waterfall
//         bullet rewritten at a35826d5, a logbook bullet split in two, six collapsed on purpose
//         at e329cbb5, a Config-tab reword). They are not 19 bugs and nothing needs fixing.
//         Never wire --all into a hook or a workflow: it would be red on a healthy tree, and a
//         gate that is red on a healthy tree is one people learn to push past.
//
// OVERRIDES (both also documented in .githooks/pre-push's header, beside every other override):
//       NEXUS_ALLOW_CHANGELOG_LOSS=1   a DELIBERATE removal — a revert that correctly dropped its
//                                      own bullet. Still PRINTS every bullet it lets through, so
//                                      the decision lands on the record; exits 0.
//       NEXUS_SKIP_CHANGELOG_MERGE=1   skip check 7 in the hook entirely. The blunt instrument;
//                                      prefer the one above, which leaves a record of what it let
//                                      past.

import { execFileSync } from 'node:child_process';

const FILE = 'CHANGELOG.md';

function git(args) {
  return execFileSync('git', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
}

function changelogAt(sha) {
  try {
    return git(['show', `${sha}:${FILE}`]);
  } catch {
    return null;
  }
}

// A bullet's identity is its BOLD HEADING — `- **Like this.**` — normalised.
//
// Not the whole first paragraph: measured against real history, keying on the paragraph made a
// body reword read as a loss, which fired six times in one release cycle on one innocent edit
// (5ac3a28b and five descendants, where a branch changed "readings stop" to "readings arriving"
// ninety characters in). The heading is what identifies a bullet to a reader, and it is the part
// that survives ordinary editing. A bullet with no bold heading falls back to its first line.
function bulletKeys(text) {
  const keys = new Set();
  for (const line of text.split('\n')) {
    if (!/^- \S/.test(line)) continue;
    const body = line.slice(2);
    const bold = body.match(/^\*\*(.+?)\*\*/);
    const key = (bold ? bold[1] : body)
      .replace(/[*_`]/g, '')
      .replace(/[^\p{L}\p{N} ]/gu, '')
      .replace(/\s+/g, ' ')
      .trim()
      .toLowerCase()
      .slice(0, 80);
    if (key.length >= 12) keys.add(key);   // too short to identify anything
  }
  return keys;
}

// A heading that gained or shed a trailing qualifier is the same bullet, not a new one. Real
// case: 8fae506c turned "The FT-710 can draw its own band scope." into "The FT-710 can draw its
// own band scope — in source builds only." — a deliberate correction, and without this it reads
// as a loss at every later merge.
function survives(key, present) {
  if (present.has(key)) return true;
  for (const candidate of present) {
    if (candidate.startsWith(key) || key.startsWith(candidate)) return true;
  }
  return false;
}

// `git merge -s ours` records ancestry and deliberately keeps the first parent's tree. Nexus uses
// it to bridge the SourceForge history (18f1f049). Detected by comparing trees, never by SHA, so
// the next one is exempt too.
function isOursMerge(sha, parents) {
  try {
    return git(['rev-parse', `${sha}^{tree}`]).trim()
      === git(['rev-parse', `${parents[0]}^{tree}`]).trim();
  } catch {
    return false;
  }
}

// The tip of a range: `A..B` → B, a bare rev → itself. Used to ask "is this bullet STILL gone?".
function tipOf(range) {
  const token = range.trim().split(/\s+/).pop();
  const rev = token.includes('..') ? token.split('..').pop() : token;
  return git(['rev-parse', rev || 'HEAD']).trim();
}

function defaultRange() {
  try {
    const tag = git(['describe', '--tags', '--abbrev=0', 'HEAD']).trim();
    return `${tag}..HEAD`;
  } catch {
    return 'HEAD';
  }
}

function lossesAt(sha) {
  const parents = git(['rev-parse', `${sha}^@`]).split('\n').filter(Boolean);
  if (parents.length < 2) return null;
  if (isOursMerge(sha, parents)) return { lost: [], ours: true };

  const mergeText = changelogAt(sha);
  if (mergeText === null) {
    const anyParent = parents.some((p) => changelogAt(p) !== null);
    return anyParent ? { skip: `${FILE} absent at the merge but present in a parent` } : null;
  }

  const present = bulletKeys(mergeText);
  const lost = new Map();
  for (const p of parents) {
    const text = changelogAt(p);
    if (text === null) continue;
    for (const key of bulletKeys(text)) {
      if (!survives(key, present) && !lost.has(key)) {
        lost.set(key, { key, ...labelFor(text, key) });
      }
    }
  }
  return { lost: [...lost.values()] };
}

// The label AND the release section the bullet lived in at that parent. The section is what turns
// "28 bullets went missing" into the question that actually matters — whether a SHIPPED release was
// advertised incomplete, or only the one being prepared.
function labelFor(text, key) {
  let section = '(no section)';
  for (const line of text.split('\n')) {
    const heading = line.match(/^## \[([^\]]+)\]/);
    if (heading) section = heading[1];
    if (!/^- \S/.test(line)) continue;
    const bold = line.slice(2).match(/^\*\*(.+?)\*\*/);
    const norm = (bold ? bold[1] : line.slice(2))
      .replace(/[*_`]/g, '').replace(/[^\p{L}\p{N} ]/gu, '')
      .replace(/\s+/g, ' ').trim().toLowerCase().slice(0, 80);
    // A bold heading that wraps across two lines never closes on the first, so `bold` is null
    // and the key falls back to the whole first line. Strip the dangling marker for the report.
    if (norm === key) {
      return { text: (bold ? bold[1] : line.slice(2)).replace(/\*\*/g, '').trim().slice(0, 72), section };
    }
  }
  return { text: key.slice(0, 72), section };
}

// A shallow clone has no merge history to compare, so every check below would find nothing and
// the run would print "no merge lost a bullet" and exit 0. That is a FALSE GREEN — the single
// worst outcome for a gate, and this project has shipped one before (1.10.3, a piped exit code).
// actions/checkout defaults to depth 1, so this is the ordinary CI shape, not an exotic one.
// Refuse instead, and name the fix.
function refuseIfShallow() {
  let shallow = false;
  try {
    shallow = git(['rev-parse', '--is-shallow-repository']).trim() === 'true';
  } catch {
    return;                                    // not a repo; check 0 below reports that
  }
  if (!shallow) return;
  console.error('check-changelog-merges: this is a SHALLOW clone.');
  console.error('There is no merge history here to compare, so a pass would mean nothing.');
  console.error('In CI, give the checkout step `fetch-depth: 0`. Refusing rather than');
  console.error('reporting a green that was never earned.');
  process.exit(2);
}

function checkRange(range) {
  refuseIfShallow();
  let merges;
  try {
    merges = git(['rev-list', '--merges', ...range.split(' ').filter(Boolean)])
      .split('\n').filter(Boolean);
  } catch (e) {
    console.error(`check-changelog-merges: cannot list merges for "${range}"`);
    console.error(String(e.message || e).split('\n')[0]);
    return 2;
  }

  // A bullet dropped by a merge and PUT BACK afterwards is not an outstanding loss, and reporting
  // it forever would leave this gate permanently red for the rest of a release cycle the moment it
  // did its job — which is how a check earns a --no-verify. So findings are filtered against the
  // tip of the range: what this reports is "a bullet is missing NOW", not "a merge once dropped
  // one". It loses no protection. A push that drops a bullet is judged at the pushed tip, where
  // the bullet is genuinely absent, and still goes red.
  let tipKeys;
  try {
    const tipText = changelogAt(tipOf(range));
    tipKeys = tipText === null ? null : bulletKeys(tipText);
  } catch {
    tipKeys = null;
  }

  const findings = [];
  const skipped = [];
  for (const m of merges) {
    const r = lossesAt(m);
    if (!r) continue;
    if (r.skip) { skipped.push({ sha: m, why: r.skip }); continue; }
    const outstanding = tipKeys === null
      ? r.lost
      : r.lost.filter((l) => !survives(l.key, tipKeys));
    if (outstanding.length) findings.push({ sha: m, lost: outstanding });
  }

  console.log(`check-changelog-merges: ${merges.length} merge(s) in ${range}`);
  for (const f of findings) {
    const subject = git(['log', '-1', '--format=%s', f.sha]).trim();
    console.log(`\nLOST ${f.lost.length} bullet(s) at ${f.sha.slice(0, 8)}  ${subject}`);
    for (const l of f.lost) console.log(`    [${l.section}] ${l.text}`);
  }
  for (const s of skipped) console.log(`SKIPPED ${s.sha.slice(0, 8)} — ${s.why}`);

  if (findings.length) {
    const all = findings.flatMap((f) => f.lost);
    const sections = [...new Set(all.map((l) => l.section))].sort();
    // A bullet lost from a VERSIONED section was advertised as part of a release that has already
    // shipped. One lost from [Unreleased] only affects the release being prepared. The difference
    // is the whole conversation, so it is stated rather than left to be worked out from the list.
    const shipped = sections.filter((s) => s !== 'Unreleased' && s !== '(no section)');
    console.log(`\ncheck-changelog-merges: ${all.length} bullet(s) lost across ${findings.length} merge(s).`);
    console.log(`Release section(s) affected: ${sections.join(', ')}`);
    if (shipped.length) {
      console.log(`*** ${shipped.length} ALREADY-RELEASED section(s) affected: ${shipped.join(', ')}`);
      console.log('*** Those releases were advertised incomplete. Check their published notes too.');
    }
    console.log('\nA merge may not delete a CHANGELOG bullet. Recover each from the commit that');
    console.log(`wrote it:  git log -S"<its heading>" -- ${FILE}   then  git show <sha> -- ${FILE}`);
    console.log('\nIf a removal was DELIBERATE — a revert that correctly dropped its own bullet —');
    console.log('re-run with NEXUS_ALLOW_CHANGELOG_LOSS=1. That is visible and leaves a trace;');
    console.log('there is no silent bypass, because an accidental loss looks identical to a');
    console.log('deliberate one and only you know which this is.');
    if (process.env.NEXUS_ALLOW_CHANGELOG_LOSS === '1') {
      console.log('\nNEXUS_ALLOW_CHANGELOG_LOSS=1 — the loss above is ALLOWED for this run.');
      return 0;
    }
    return 1;
  }
  if (skipped.length) {
    console.log(`\ncheck-changelog-merges: PASSED, but ${skipped.length} merge(s) not covered.`);
    return 3;
  }
  console.log('check-changelog-merges: no merge lost a bullet.');
  return 0;
}

// Both directions, against real history. A checker only ever seen green proves nothing — this
// project shipped a faked green in 1.10.3 exactly that way. The last four cases pin the two
// false-positive classes that were found by measurement and fixed; without them a later
// "simplification" of the key silently brings the noise back.
// Every case below is pinned to a REAL commit in this repository's history, because a fixture the
// tool authors themselves can shape proves only that the tool agrees with itself.
//
// The consequence, stated rather than hidden: a clone that lacks one of those commits cannot run
// that case. Shallow clones, and any clone taken before the 1.13.0 integration reaches main (which
// is where the repair commit lives), are both in that position. A missing commit is therefore
// reported and downgrades the run to exit 3 — "passed, but coverage is incomplete" — NOT to a
// failure. Failing would make this red on a healthy fresh clone, and a check that is red on a
// healthy tree is one people switch off; a gate pinned to a fixture that is not there yet blocked
// two releases in this project already.
function selftest() {
  let bad = 0;
  let uncovered = 0;
  const must = (name, ok, detail) => {
    console.log(`${ok ? 'ok  ' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`);
    if (!ok) bad++;
  };
  const skip = (name, why) => {
    console.log(`skip  ${name} — ${why}`);
    uncovered++;
  };
  const have = (sha) => {
    try { git(['rev-parse', `${sha}^{commit}`]); return true; } catch { return false; }
  };
  const count = (sha) => {
    if (!have(sha)) return null;
    const r = lossesAt(sha);
    return r ? (r.lost ? r.lost.length : -1) : -1;
  };

  // RED: merges that really did lose bullets.
  for (const [sha, n] of [['aa67f526', 6], ['e3b34846', 8], ['f3947003', 6], ['0a38477f', 1]]) {
    const got = count(sha);
    if (got === null) skip(`red on ${sha} (a real historical loss)`, 'commit not in this clone');
    else must(`red on ${sha} (a real historical loss)`, got === n, `found ${got}, expected ${n}`);
  }
  // GREEN: merges that kept everything. Without these the check could always say "lost".
  for (const sha of ['810dccb6', '9d588335', '2a82d76b']) {
    const got = count(sha);
    if (got === null) skip(`green on ${sha} (a merge that kept everything)`, 'commit not in this clone');
    else must(`green on ${sha} (a merge that kept everything)`, got === 0, `found ${got}, expected 0`);
  }
  // GREEN: the two false-positive classes, pinned.
  if (count('5ac3a28b') === null) skip('green on 5ac3a28b (a branch reworded a bullet BODY)', 'commit not in this clone');
  else must('green on 5ac3a28b (a branch reworded a bullet BODY)', count('5ac3a28b') === 0, `found ${count('5ac3a28b')}`);
  if (count('4e0d05f3') === null) skip('green on 4e0d05f3 (a heading gained a trailing qualifier)', 'commit not in this clone');
  else must('green on 4e0d05f3 (a heading gained a trailing qualifier)', count('4e0d05f3') === 0, `found ${count('4e0d05f3')}`);
  if (count('18f1f049') === null) skip('green on 18f1f049 (a `-s ours` history bridge)', 'commit not in this clone');
  else must('green on 18f1f049 (a `-s ours` history bridge)', count('18f1f049') === 0, `found ${count('18f1f049')}`);
  // A loss that has since been REPAIRED must stop being reported, or the gate stays red for the
  // rest of the release cycle the moment it does its job — and a permanently red gate is one
  // people learn to push past. 91b1f781 restored the 28 bullets those six merges dropped, so a
  // range ending at or after it must be green while a range ending BEFORE it must still be red.
  {
    const run = (range) => {
      try {
        execFileSync(process.execPath, [process.argv[1], range], { encoding: 'utf8' });
        return 0;
      } catch (e) { return e.status; }
    };
    if (!have('91b1f781')) {
      skip('repaired losses stop being reported',
        'the restore commit is not in this clone yet (it reaches main with 1.13.0)');
    } else {
      must('red on a range that ends BEFORE the repair', run('v1.12.0..91b1f781^') === 1);
      must('green on a range that INCLUDES the repair', run('v1.12.0..91b1f781') === 0);
    }
  }

  // The override must actually release the brake — and must not be on by default.
  {
    // Pinned to a range that ends BEFORE the repair, so this stays a real red once the restore
    // reaches origin/main and every later range is legitimately green.
    const range = 'v1.12.0..91b1f781^';
    if (!have('91b1f781')) {
      skip('override cases', 'the restore commit is not in this clone yet');
    } else {
    const run = (env) => {
      try {
        execFileSync(process.execPath, [process.argv[1], range],
          { encoding: 'utf8', env: { ...process.env, ...env } });
        return 0;
      } catch (e) { return e.status; }
    };
    must('blocks by default on a range with a real loss', run({ NEXUS_ALLOW_CHANGELOG_LOSS: '' }) === 1);
    must('NEXUS_ALLOW_CHANGELOG_LOSS=1 lets that same range through',
      run({ NEXUS_ALLOW_CHANGELOG_LOSS: '1' }) === 0);
    }
  }

  // A bullet must be attributed to the release section it lived in, or the report cannot tell a
  // shipped release advertised incomplete from the one being prepared.
  {
    if (!have('aa67f526')) { skip('losses are attributed to a release section', 'commit not in this clone'); }
    else {
    const r = lossesAt(git(['rev-parse', 'aa67f526']).trim());
    const sections = new Set(r.lost.map((l) => l.section));
    must('losses are attributed to a release section',
      sections.size === 1 && sections.has('Unreleased'), `got ${[...sections].join(',')}`);
    }
  }

  // The extractor must count, not merely run.
  const two = bulletKeys('- **Alpha bravo charlie.** body\n- **Delta echo foxtrot.** body\n');
  must('extractor finds two bullets in a two-bullet sample', two.size === 2, `got ${two.size}`);
  must('a re-wrapped body does not change a bullet key',
    [...bulletKeys('- **Alpha bravo charlie.** one\n  two\n')][0]
    === [...bulletKeys('- **Alpha bravo charlie.** one two\n')][0]);

  if (bad) {
    console.log(`\nselftest: ${bad} FAILED`);
    return 1;
  }
  if (uncovered) {
    console.log(`\nselftest: passed, but ${uncovered} case(s) could not run in this clone.`);
    console.log('Exit 3 — coverage incomplete, not a failure. A shallow clone, or one taken before');
    console.log('the 1.13.0 integration reached main, legitimately lacks the pinned commits.');
    return 3;
  }
  console.log('\nselftest: all checks passed');
  return 0;
}

const argv = process.argv.slice(2);
if (argv.includes('--selftest')) process.exit(selftest());
const positional = argv.filter((a) => !a.startsWith('--'));
const range = argv.includes('--all') ? 'HEAD' : (positional.join(' ') || defaultRange());
process.exit(checkRange(range));
