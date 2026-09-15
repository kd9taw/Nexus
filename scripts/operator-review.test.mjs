// Tests for scripts/operator-review.
//
// EVERY ASSERTION HERE IS PAIRED WITH A CONTROL THAT MUST TRIP. That is not a
// style choice in this file of all files: the tool under test exists because
// this project keeps accepting a reassuring "nothing found", and a test suite
// for it that could not go red would be the same defect one level up. So each
// "it fired" is preceded by "it did NOT fire before", and each "it passed" by a
// planted change that makes it fail.
//
// The properties under test, in the order they matter:
//
//   1. The path map is DATA. A glob added to the file changes the verdict with
//      no edit to the script. The moment that stops being true the map is a
//      transcription living in two places, which is the defect scripts/gates was
//      written to kill.
//   2. The gate fires, and does not fire. Both directions, on real commits from
//      this repository's history.
//   3. The calibration corpus can go RED. Proven by removing one glob and
//      watching the two cases the tool was built around fail.
//   4. A finding with no closure is refused. That single rule is the whole
//      point of the receipt.
//   5. Advisory is advisory and binding binds, on identical input.
//   6. A data file this tool cannot read in full is a hard error naming the
//      line, never a quiet pass over the part it managed to read.
//
// Run: node --test scripts/operator-review.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TOOL = path.join(ROOT, 'scripts', 'operator-review');
const DATA = path.join(ROOT, 'scripts', 'operator-review.d');
const MAP = path.join(DATA, 'paths.map');

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'opreview-test-'));
const scratch = (name, text) => {
  const p = path.join(tmp, name);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, text);
  return p;
};

function run(args) {
  const r = spawnSync(TOOL, args, { encoding: 'utf8', cwd: ROOT });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
}

// Real commits from this repository, used as fixtures. If one of these ever
// stops resolving the test says so rather than quietly passing on an empty
// diff — see the guard in the first test.
const RISKY = '636ad7e3~1..636ad7e3'; // the CAT auto-reconnect that stranded a keyed Yaesu
const DOCS_ONLY = '7556e2b1~1..7556e2b1'; // a CHANGELOG-only commit

test('the fixture commits still resolve — without this every test below passes vacuously', () => {
  for (const range of [RISKY, DOCS_ONLY]) {
    const r = spawnSync('git', ['-C', ROOT, 'diff', '--name-only', range], { encoding: 'utf8' });
    assert.equal(r.status, 0, `${range} does not resolve in this repository`);
    assert.ok(r.stdout.trim().length > 0, `${range} resolved to an EMPTY diff — the fixture has rotted`);
  }
});

// ---------------------------------------------------------------------------
// 1. The path map is data, not a list inside the script.
// ---------------------------------------------------------------------------

test('a glob added to the path map changes the verdict, with no edit to the script', () => {
  const victim = 'README.md'; // deliberately something no lens claims
  const files = scratch('files-readme.txt', `${victim}\n`);

  // CONTROL: it must NOT require a review before the glob is planted. If this
  // assertion cannot fail, the one after it proves nothing.
  const before = run(['--files-from', files]);
  assert.equal(before.code, 0);
  assert.match(before.out, /NO REVIEW REQUIRED/, 'control failed: README.md already required a review');

  const planted = scratch(
    'planted.map',
    fs.readFileSync(MAP, 'utf8') + '\ntransmit | README.md | planted by the test suite\n'
  );
  const after = run(['--files-from', files, '--path-map', planted]);
  assert.match(after.out, /REVIEW REQUIRED/, 'a glob added to the map did not reach the verdict');
  assert.match(after.out, /planted by the test suite/, 'the reason from the map is not carried into the report');
});

test('a lens used before it is declared is refused by name, not invented silently', () => {
  const planted = scratch('undeclared.map', fs.readFileSync(MAP, 'utf8') + '\nbogus | README.md | nope\n');
  const r = run(['--lenses', '--path-map', planted]);
  assert.equal(r.code, 2, 'an undeclared lens must be a hard error');
  assert.match(r.out, /lens `bogus` is used before it is declared/);
});

// ---------------------------------------------------------------------------
// 2. The gate fires, and does not fire.
// ---------------------------------------------------------------------------

test('a change to a transmit/CAT path requires a review', () => {
  const r = run(['--range', RISKY]);
  assert.match(r.out, /REVIEW REQUIRED/);
  assert.match(r.out, /transmit/);
  assert.match(r.out, /cat-rig/);
  // The scenarios are the point of requiring it — a lens with nothing behind it
  // is a prompt to look at nothing.
  assert.match(r.out, /scenarios to review against/);
});

test('a change that touches no path in the map requires nothing — and says what that rests on', () => {
  const r = run(['--range', DOCS_ONLY]);
  assert.equal(r.code, 0);
  assert.match(r.out, /NO REVIEW REQUIRED/);
  // The honest failure mode of a path map is a confident silence, so the
  // "no" must carry its own caveat. This is a real assertion, not decoration:
  // the verdict most likely to be wrong is this one.
  assert.match(r.out, /THE MAP IS WRONG/);
});

test('every lens the path map declares has at least one scenario behind it', () => {
  const r = run(['--check-data']);
  assert.equal(r.code, 0, `--check-data went red:\n${r.out}`);
  assert.match(r.out, /DATA OK/);
});

test('--check-data refuses a glob that matches no tracked file', () => {
  const planted = scratch(
    'rotted.map',
    fs.readFileSync(MAP, 'utf8') + '\ntransmit | crates/tempo-app/src/no-such-file.rs | planted rot\n'
  );
  const r = run(['--check-data', '--path-map', planted]);
  assert.equal(r.code, 2, 'a rotted glob must not pass --check-data');
  assert.match(r.out, /matches no tracked file/);
});

// ---------------------------------------------------------------------------
// 3. The calibration corpus, and its positive control.
// ---------------------------------------------------------------------------

test('the calibration corpus routes every case it can route', () => {
  const r = run(['--calibrate']);
  // 0 = fully green, 3 = green on everything routable with some case declaring
  // itself uncalibratable. 1 is a RED case and is the failure.
  assert.notEqual(r.code, 1, `calibration went RED:\n${r.out}`);
  assert.match(r.out, /green {2}stuck-tx-cat-teardown/);
  assert.match(r.out, /green {2}stale-decode-wrong-parity-tx/);
  assert.match(r.out, /green {2}phone-sideband-missing/);
});

test('CONTROL: removing one glob makes the calibration go RED', () => {
  // Drop every row naming the audio service — the file the stuck-transmit and
  // the stale-decode defects were both introduced in. A calibration that stays
  // green through this is not measuring anything.
  const planted = scratch(
    'no-service.map',
    fs
      .readFileSync(MAP, 'utf8')
      .split('\n')
      .filter((l) => !l.includes('crates/tempo-audio/src/service.rs'))
      .join('\n')
  );
  const r = run(['--calibrate', '--path-map', planted]);
  assert.equal(r.code, 1, `the calibration did NOT go red with the glob removed:\n${r.out}`);
  assert.match(r.out, /RED {4}stuck-tx-cat-teardown/);
  assert.match(r.out, /RED {4}stale-decode-wrong-parity-tx/);
  assert.match(r.out, /MISSING {6}: transmit/);
  // And it must say to fix the map rather than the corpus, or the next person
  // closes it the cheap way.
  assert.match(r.out, /Fix the MAP \(add the glob\), not the corpus/);
});

test('a calibration case with no introducing commit is reported, not quietly scored', () => {
  const r = run(['--calibrate']);
  assert.match(r.out, /UNCAL/);
  assert.match(r.out, /uncalibratable/);
});

// ---------------------------------------------------------------------------
// 4. The receipt. A finding with no closure is the failure this tool exists for.
// ---------------------------------------------------------------------------

const RECEIPT_BODY = [
  'range: ' + RISKY,
  'lens: transmit, cat-rig',
  'reviewed: 2026-09-15',
  'finding: R1 | on-air | the rebuild does not carry the keyed belief across the reconnect',
  'closure: bench | BENCH-01 — Yaesu Enhanced port: key a tune, pull USB, watch the radio unkey',
  '',
].join('\n');

const receiptDir = (name, body) => {
  const dir = path.join(tmp, name);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, '636ad7e3-1-636ad7e3.txt'), body);
  return dir;
};

test('a receipt whose findings are all closed closes the review, even binding', () => {
  const dir = receiptDir('ok', RECEIPT_BODY);
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /REVIEW COMPLETE/);
});

test('CONTROL: a finding with its closure removed is refused', () => {
  const dir = receiptDir('unclosed', RECEIPT_BODY.split('\n').filter((l) => !l.startsWith('closure:')).join('\n'));
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 1, 'an unclosed finding must refuse');
  assert.match(r.out, /finding has NO closure/);
});

test('a closure kind that is not one of the four is refused by name', () => {
  const dir = receiptDir('badkind', RECEIPT_BODY.replace('closure: bench |', 'closure: noted |'));
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 1);
  assert.match(r.out, /closure kind `noted` is not one of: fixed, test, bench, accepted/);
});

test('a closure with no detail is refused — naming the test, the bench step or the reason IS the closure', () => {
  const dir = receiptDir('nodetail', RECEIPT_BODY.replace(/closure: bench \|.*/, 'closure: bench |'));
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 1);
  assert.match(r.out, /carries no detail/);
});

test('an empty receipt is not a clean review — "no findings" has to be written down', () => {
  const dir = receiptDir('empty', 'range: ' + RISKY + '\nlens: transmit, cat-rig\nreviewed: 2026-09-15\n');
  const bad = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(bad.code, 1, 'an empty receipt must not pass as a review');
  assert.match(bad.out, /does not say `findings: none`/);

  // And the deliberate version passes, so the rule is a demand for a sentence
  // rather than a ban on the result.
  const good = receiptDir('none', 'range: ' + RISKY + '\nlens: transmit, cat-rig\nfindings: none\n');
  const ok = run(['--binding', '--range', RISKY, '--receipts-dir', good]);
  assert.equal(ok.code, 0, ok.out);
  assert.match(ok.out, /findings: none — recorded deliberately/);
});

test('a receipt that does not cover a required lens is refused', () => {
  const dir = receiptDir('onelens', RECEIPT_BODY.replace('lens: transmit, cat-rig', 'lens: transmit'));
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 1);
  assert.match(r.out, /does not record the `cat-rig` lens/);
});

test('a typo in a receipt key is named, not read as a receipt with no findings', () => {
  const dir = receiptDir('typo', RECEIPT_BODY.replace('finding: R1', 'findng: R1'));
  const r = run(['--binding', '--range', RISKY, '--receipts-dir', dir]);
  assert.equal(r.code, 1);
  assert.match(r.out, /unknown key `findng`/);
});

// ---------------------------------------------------------------------------
// 5. Advisory is advisory; binding binds. Same input, two exit codes.
// ---------------------------------------------------------------------------

test('advisory reports and exits 0; binding refuses with 1, on identical input', () => {
  const advisory = run(['--range', RISKY]);
  assert.equal(advisory.code, 0, 'advisory must not fail a build today');
  assert.match(advisory.out, /ADVISORY/);
  // It has to say what it WOULD have done, or advisory mode is indistinguishable
  // from the tool not being wired up.
  assert.match(advisory.out, /IN BINDING MODE THIS WOULD HAVE EXITED 1/);

  const binding = run(['--binding', '--range', RISKY]);
  assert.equal(binding.code, 1);
  assert.match(binding.out, /BINDING — refusing/);
});

test('the binding switch is one env var as well as one flag', () => {
  const r = spawnSync(TOOL, ['--range', RISKY], {
    encoding: 'utf8',
    cwd: ROOT,
    env: { ...process.env, NEXUS_OPREVIEW_BINDING: '1' },
  });
  assert.equal(r.status, 1, 'NEXUS_OPREVIEW_BINDING=1 must bind');
});

// ---------------------------------------------------------------------------
// 6. Loud, never silent.
// ---------------------------------------------------------------------------

test('a malformed path-map line is a hard error naming the line number', () => {
  const planted = scratch('malformed.map', fs.readFileSync(MAP, 'utf8') + '\ntransmit | only-two-fields\n');
  const r = run(['--lenses', '--path-map', planted]);
  assert.equal(r.code, 2);
  assert.match(r.out, /:\d+: expected `lens \| glob \| why`/);
});

test('an unknown flag is a usage error, not a silently ignored argument', () => {
  const r = run(['--not-a-flag']);
  assert.equal(r.code, 2);
  assert.match(r.out, /unknown argument --not-a-flag/);
});

test('--brief prints the reviewer prompt AND the scenarios for the lenses this change needs', () => {
  const r = run(['--brief', '--range', RISKY]);
  assert.equal(r.code, 0);
  assert.match(r.out, /the reviewer's brief/);
  // The two rules a reviewer most often drops.
  assert.match(r.out, /A finding with no citation is filed as a QUESTION/);
  assert.match(r.out, /NEEDS THE OPERATOR'S BENCH/);
  // And the scenarios actually arrive, rather than the brief pointing at a file.
  assert.match(r.out, /\[cat-teardown-while-keyed\]/);
});
