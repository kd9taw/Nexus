// Tests for scripts/gates.
//
// The property under test is the ONE the tool rests on: the gate list is DERIVED
// from .github/workflows/ci.yml, not remembered. A step added to the workflow has
// to appear with no edit to the script — because the moment it doesn't, `gates`
// is just another stale transcription, which is the defect it was written to
// kill.
//
// Every "it appeared" assertion here is paired with a control that MUST fail if
// the check is broken: the planted step is asserted ABSENT from the unmodified
// workflow first. A green run of an assertion that cannot go red proves nothing.
//
// Run: node --test scripts/gates.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const GATES = path.join(ROOT, 'scripts', 'gates');
const CI = path.join(ROOT, '.github', 'workflows', 'ci.yml');

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'gates-test-'));
const scratch = (name, text) => {
  const p = path.join(tmp, name);
  fs.writeFileSync(p, text);
  return p;
};

function gates(args) {
  const r = spawnSync(GATES, args, { encoding: 'utf8', cwd: ROOT });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
}

// The summary line, not the per-job banners — they use the same words.
const gateCount = (s) => Number(s.match(/(\d+) gate\(s\); \d+ cannot run here/)[1]);

const MINIMAL = `name: scratch
on: [push]
jobs:
  alpha:
    name: Alpha
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v7
      - name: alpha gate
        run: echo alpha-ran
`;

// ---------------------------------------------------------------------------
// The derivation property, with its control.
// ---------------------------------------------------------------------------

test('a step added to the workflow appears in --list, with no edit to the script', () => {
  const original = fs.readFileSync(CI, 'utf8');

  // CONTROL: the planted text must be absent before it is planted. If this
  // assertion cannot fail, the one after it means nothing.
  const before = gates(['--list']);
  assert.equal(before.code, 0);
  assert.ok(
    !before.out.includes('planted-gate-marker'),
    'control failed: the marker was already in the real workflow listing'
  );

  // Plant a step at the end of the `test` job — same indentation as its siblings.
  const anchor = '      - name: cargo clippy src-tauri --features radio\n';
  assert.ok(original.includes(anchor), 'the anchor step moved; update this test');
  const planted = original.replace(
    anchor,
    '      - name: planted-gate-marker\n        run: echo planted-gate-marker\n' + anchor
  );
  const file = scratch('planted-step.yml', planted);

  const after = gates(['--list', '--workflow', file]);
  assert.equal(after.code, 0);
  assert.ok(after.out.includes('planted-gate-marker'), 'the added step did NOT appear in --list');
  assert.ok(
    after.out.includes('echo planted-gate-marker'),
    'the added step appeared without its exact command'
  );

  // And it is counted as a gate, not swallowed as setup.
  assert.equal(
    gateCount(after.out),
    gateCount(before.out) + 1,
    'the gate count did not go up by exactly one'
  );
});

test('a whole new job added to the workflow appears in --list', () => {
  const original = fs.readFileSync(CI, 'utf8');
  const file = scratch(
    'planted-job.yml',
    `${original}\n  planted-job:\n    name: Planted\n    runs-on: ubuntu-latest\n` +
      `    steps:\n      - name: planted job gate\n        run: echo planted-job-ran\n`
  );
  const out = gates(['--list', '--workflow', file]).out;
  assert.ok(out.includes('planted-job'), 'the added job did not appear');
  assert.ok(out.includes('echo planted-job-ran'), 'the added job\'s command did not appear');
});

test('every job in the real workflow is listed', () => {
  // Read the job ids straight out of the YAML by a different route than the
  // script uses, so this cannot agree with it by sharing a bug.
  const lines = fs.readFileSync(CI, 'utf8').split('\n');
  const start = lines.findIndex((l) => l === 'jobs:');
  const ids = lines
    .slice(start + 1)
    .filter((l) => /^ {2}[a-z][\w-]*:\s*$/.test(l))
    .map((l) => l.trim().replace(':', ''));
  assert.ok(ids.length >= 8, `expected the workflow to have several jobs, found ${ids.length}`);
  const out = gates(['--list']).out;
  for (const id of ids) assert.ok(out.includes(id), `job ${id} is missing from --list`);
});

// ---------------------------------------------------------------------------
// LOUD, NEVER SILENT — rule 1 of the script's header.
//
// Every test below was written from a shape that made the tool report success
// while having run LESS than it claimed. That is the exact failure the tool
// exists to eliminate, so each one asserts BOTH halves: the missing work is
// visible, AND the run does not print the full-coverage line.
// ---------------------------------------------------------------------------

const FLUSH_STEPS = `name: scratch
on: [push]
jobs:
  bravo:
    runs-on: ubuntu-latest
    steps:
    - name: flush-shape-marker
      run: echo flush-shape-ran
  alpha:
    runs-on: ubuntu-latest
    steps:
      - name: alpha gate
        run: echo alpha-ran
`;

test('a `steps:` sequence flush with its own key keeps every gate', () => {
  // Flush-indented block sequences are legal YAML. This shape used to end the
  // parse at the dash: the job vanished from --list, its gates never ran, and
  // the run still printed ALL GATES PASSED and exited 0. With the flush job
  // FIRST, as here, ZERO gates ran under that same green line.
  const flush = gates(['--list', '--workflow', scratch('flush-steps.yml', FLUSH_STEPS)]);
  assert.equal(flush.code, 0);
  assert.ok(flush.out.includes('flush-shape-marker'), 'the flush-indented job lost its step');
  assert.ok(flush.out.includes('echo flush-shape-ran'), 'the step appeared without its command');
  assert.ok(flush.out.includes('alpha gate'), 'the job AFTER the flush one was dropped');

  // CONTROL: the identical workflow written the ordinary way must give the same
  // count. Equality is the assertion — a number that agreed for the wrong reason
  // would have to be wrong in both files.
  const indented = FLUSH_STEPS.replace(
    '    steps:\n    - name: flush-shape-marker\n      run: echo flush-shape-ran\n',
    '    steps:\n      - name: flush-shape-marker\n        run: echo flush-shape-ran\n'
  );
  assert.notEqual(indented, FLUSH_STEPS, 'the re-indent did nothing; update this test');
  const plain = gates(['--list', '--workflow', scratch('indented-steps.yml', indented)]);
  assert.equal(gateCount(flush.out), gateCount(plain.out), 'the two indent styles disagree');

  const run = gates(['--workflow', scratch('flush-steps-run.yml', FLUSH_STEPS)]);
  assert.equal(run.code, 0);
  assert.ok(run.out.includes('flush-shape-ran'), "the flush job's gate never ran");
  assert.ok(run.out.includes('alpha-ran'), 'the job after the flush one never ran');
});

test('a region the YAML reader cannot account for is refused, never quietly dropped', () => {
  // The general form of the bug above: a construct that ends the parse early.
  // The reader only moves forward, so "did we reach the end of the file" is a
  // complete answer to "did we read all of it" — and anything less than the
  // whole file must not become a shorter gate list under a green summary.
  const stranded = `name: scratch
on: [push]
jobs:
  alpha:
    runs-on: ubuntu-latest
    steps:
      - name: alpha gate
        run: echo alpha-ran
  - a-shape-the-reader-cannot-place: 1
  bravo:
    runs-on: ubuntu-latest
    steps:
      - name: bravo gate
        run: echo bravo-ran
`;
  const r = gates(['--workflow', scratch('stranded.yml', stranded)]);
  assert.equal(r.code, 2, 'an unreadable workflow must not exit 0');
  assert.ok(/the YAML reader stopped at line 9/.test(r.out), `the stopping point was not named:\n${r.out}`);
  assert.ok(!r.out.includes('ALL GATES PASSED'), 'it claimed full coverage over an unread file');
  assert.ok(!r.out.includes('alpha-ran'), 'it ran gates out of a file it could not finish reading');

  // CONTROL: the same file without the stray line reads fine, so the refusal is
  // about that construct and not about the workflow in general.
  const ok = gates(['--workflow', scratch('unstranded.yml', stranded.replace('  - a-shape-the-reader-cannot-place: 1\n', ''))]);
  assert.equal(ok.code, 0);
  assert.ok(ok.out.includes('bravo-ran'));
});

test('YAML constructs the reader does not implement are refused BY NAME', () => {
  // An alias read as the literal string `*common` parses fine and means
  // something else: bravo's gate would run with none of the env CI gives it.
  const anchored = `name: scratch
on: [push]
jobs:
  alpha:
    runs-on: ubuntu-latest
    env: &common
      RUSTFLAGS: -D warnings
    steps:
      - name: alpha gate
        run: echo alpha-ran
`;
  const anchors = gates(['--workflow', scratch('anchors.yml', anchored)]);
  assert.equal(anchors.code, 2);
  assert.ok(/YAML anchor/.test(anchors.out), 'the construct was not named');
  assert.ok(!anchors.out.includes('ALL GATES PASSED'));

  // A flow mapping read as a string has "entries" that are its characters.
  const flow = gates([
    '--workflow',
    scratch('flowmap.yml', anchored.replace('env: &common\n      RUSTFLAGS: -D warnings\n', 'env: { RUSTFLAGS: -D warnings }\n')),
  ]);
  assert.equal(flow.code, 2);
  assert.ok(/flow mapping/.test(flow.out), 'the construct was not named');

  // CONTROL: the same env written as a block map is read, not refused.
  const ok = gates(['--workflow', scratch('blockmap.yml', anchored.replace('env: &common', 'env:'))]);
  assert.equal(ok.code, 0);
  assert.ok(ok.out.includes('alpha-ran'));
});

test('being loud does not mean refusing the ordinary — `workflow_dispatch: {}` is read', () => {
  // The EMPTY flow map carries no entries to misread. A blanket refusal on `{`
  // rejected four of this repo's ten workflows, which is how a strict tool gets
  // dropped rather than fixed.
  const r = gates([
    '--workflow',
    scratch('empty-flow-map.yml', MINIMAL.replace('on: [push]\n', 'on:\n  push:\n  workflow_dispatch: {}\n')),
  ]);
  assert.equal(r.code, 0, r.out);
  assert.ok(r.out.includes('alpha-ran'), 'an empty flow map stopped the workflow being read');
});

test('every workflow in this repo can be read end to end', () => {
  // The breadth check that caught the over-broad `{` refusal above. A refusal
  // here is a real signal — someone pointing scripts/gates at that workflow
  // would get one — so the fix is to teach the reader the construct, or to
  // simplify the construct, never to loosen this test.
  const dir = path.join(ROOT, '.github', 'workflows');
  const files = fs.readdirSync(dir).filter((f) => f.endsWith('.yml') || f.endsWith('.yaml'));
  assert.ok(files.length >= 5, `expected several workflows, found ${files.length}`);
  for (const f of files) {
    const r = gates(['--list', '--workflow', path.join(dir, f)]);
    assert.equal(r.code, 0, `.github/workflows/${f} could not be read:\n${r.out}`);
  }
});

test('a step that mixes provisioning with a gate keeps the gate, and never runs the installer', () => {
  // `isProvisioning` matched per LINE, so one `&&` was enough to classify a real
  // gate as setup: it left the count, never ran, and the run said ALL GATES
  // PASSED. Neither side is a fix on its own — running it installs system
  // packages behind the operator, dropping it deletes a gate — so it is a gate
  // that cannot run HERE, with the reason named.
  const mixed = MINIMAL.replace(
    '        run: echo alpha-ran\n',
    '        run: sudo apt-get install -y mixed-step-marker && echo alpha-ran\n'
  );
  const file = scratch('mixed-provisioning.yml', mixed);
  const list = gates(['--list', '--workflow', file]);
  assert.equal(gateCount(list.out), 1, 'the gate was swallowed as a provisioning step');
  assert.ok(/mixes machine provisioning with a gate/.test(list.out), 'no reason was given');

  const run = gates(['--workflow', file]);
  assert.notEqual(run.code, 0, 'a dropped gate must not leave a green exit');
  assert.ok(!run.out.includes('ALL GATES PASSED'));
  assert.ok(!run.out.includes('alpha-ran'), 'the mixed step was executed');
  assert.ok(!run.out.includes('mixed-step-marker\n'), 'apt-get was invoked');

  // CONTROL: `set -euo pipefail` opens half the real workflow's steps and is
  // shell bookkeeping, not provisioning — it must NOT make a step mixed.
  const setE = gates([
    '--list',
    '--workflow',
    scratch('set-e.yml', MINIMAL.replace('        run: echo alpha-ran\n', '        run: |\n          set -euo pipefail\n          echo alpha-ran\n')),
  ]);
  assert.equal(gateCount(setE.out), 1);
  assert.ok(!/mixes machine provisioning/.test(setE.out), '`set -e` was read as an installer');
});

test('workflow-level and job-level env, and working-directory, reach the gate', () => {
  // Dropped without a word before this. A job-level RUSTFLAGS or feature var
  // makes the local gate STRICTLY WEAKER than CI's while looking identical.
  const file = scratch(
    'env-layers.yml',
    `name: scratch
on: [push]
env:
  WORKFLOW_LEVEL: wf-value
jobs:
  alpha:
    runs-on: ubuntu-latest
    env:
      RUSTFLAGS: -D warnings
    defaults:
      run:
        working-directory: ui
    steps:
      - name: alpha gate
        run: echo "seen [$WORKFLOW_LEVEL] [$RUSTFLAGS] $(basename "$(pwd)")"
`
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 0, r.out);
  assert.ok(
    r.out.includes('seen [wf-value] [-D warnings] ui'),
    `the gate did not run with CI's environment or directory:\n${r.out}`
  );
  const list = gates(['--list', '--workflow', file]).out;
  assert.ok(/RUSTFLAGS=-D warnings/.test(list), 'the job-level env was not shown');
  assert.ok(/working-directory: ui/.test(list), 'the working-directory was not shown');
});

test('`if:` is shown, and one that cannot be evaluated blocks the step instead of guessing', () => {
  const file = scratch(
    'conditional.yml',
    MINIMAL.replace(
      '      - name: alpha gate\n',
      "      - name: conditional gate\n        if: github.event_name == 'push'\n        run: echo conditional-ran\n      - name: alpha gate\n"
    )
  );
  const list = gates(['--list', '--workflow', file]).out;
  assert.ok(/if: github\.event_name == 'push'/.test(list), 'the `if:` was not shown at all');

  const run = gates(['--workflow', file]);
  assert.notEqual(run.code, 0, 'a step that could not be evaluated must not leave a green exit');
  assert.ok(!run.out.includes('conditional-ran'), 'an unevaluable `if:` was guessed as true');
  assert.ok(!run.out.includes('ALL GATES PASSED'));
  assert.ok(run.out.includes('SKIPPED'), 'the skipped step was not reported');

  // CONTROL: an `if:` this script CAN evaluate does not block anything.
  const always = gates([
    '--workflow',
    scratch('always.yml', MINIMAL.replace('      - name: alpha gate\n', '      - name: alpha gate\n        if: always()\n')),
  ]);
  assert.equal(always.code, 0, always.out);
  assert.ok(always.out.includes('alpha-ran'));
});

test('a step key this script does not implement is reported, not ignored', () => {
  const file = scratch(
    'strange-key.yml',
    MINIMAL.replace('        run: echo alpha-ran\n', '        run: echo alpha-ran\n        some-future-github-key: matters\n')
  );
  const list = gates(['--list', '--workflow', file]).out;
  assert.ok(/some-future-github-key/.test(list), 'the unimplemented key was dropped in silence');
  assert.ok(list.includes('UNKNOWN'));
  const run = gates(['--workflow', file]);
  assert.notEqual(run.code, 0);
  assert.ok(!run.out.includes('ALL GATES PASSED'));

  // A `shell:` that is not bash is the same class: this script runs bash.
  const pwsh = gates([
    '--list',
    '--workflow',
    scratch('pwsh.yml', MINIMAL.replace('        run: echo alpha-ran\n', '        shell: pwsh\n        run: Write-Host hi\n')),
  ]).out;
  assert.ok(/shell: pwsh/.test(pwsh), 'a non-bash shell was run as if it were bash');
});

// ---------------------------------------------------------------------------
// Exit codes. The 1.10.3 defect was a gate whose failure was swallowed by a pipe.
// ---------------------------------------------------------------------------

test('a passing gate exits 0 and says so', () => {
  const file = scratch('pass.yml', MINIMAL);
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 0);
  assert.ok(r.out.includes('alpha-ran'), 'the gate did not actually run');
  assert.ok(r.out.includes('ALL GATES PASSED'));
});

test('a failing gate is reported with its REAL exit code and stops success', () => {
  const file = scratch(
    'fail.yml',
    MINIMAL.replace('        run: echo alpha-ran\n', '        run: exit 7\n')
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 1, 'a red gate must make the script exit non-zero');
  assert.ok(r.out.includes('exit 7'), 'the gate exit code was not reported verbatim');
  assert.ok(r.out.includes('GATES FAILED'));
  assert.ok(!r.out.includes('ALL GATES PASSED'));
});

test('one red gate among several still fails the run', () => {
  const file = scratch(
    'mixed.yml',
    `${MINIMAL}      - name: bravo gate
        run: exit 3
      - name: charlie gate
        run: echo charlie-ran
`
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 1);
  assert.ok(r.out.includes('charlie-ran'), 'a gate after the red one was skipped');
  assert.ok(/passed 2 {2}failed 1/.test(r.out), `summary line wrong:\n${r.out}`);
});

test('a multi-line gate stops at its first failure (GitHub `bash -e` semantics)', () => {
  // A marker on disk, not in the output: the runner echoes each command before
  // running it, so the text of the second line is in stdout either way.
  const marker = path.join(tmp, 'second-line-ran');
  const file = scratch(
    'multiline.yml',
    MINIMAL.replace('        run: echo alpha-ran\n', `        run: |\n          false\n          touch ${marker}\n`)
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 1);
  assert.ok(!fs.existsSync(marker), 'the step continued past a failing line');
});

// ---------------------------------------------------------------------------
// Classification: what runs, what does not, and what is never silently dropped.
// ---------------------------------------------------------------------------

test('a machine-provisioning step is listed but never executed', () => {
  const file = scratch(
    'provision.yml',
    MINIMAL.replace(
      '      - name: alpha gate\n',
      '      - name: install things\n        run: sudo apt-get install -y cowsay-marker\n' +
        '      - name: alpha gate\n'
    )
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 0);
  assert.ok(!r.out.includes('=== RUN   alpha[install things]'), 'a provisioning step was run');
  assert.ok(!r.out.includes('cowsay-marker\n'), 'apt-get was invoked');
  const list = gates(['--list', '--workflow', file]).out;
  assert.ok(/setup\s+install things/.test(list), 'the provisioning step vanished from --list');
});

test('an action with no local equivalent is reported, never silently dropped', () => {
  const file = scratch(
    'unknown-action.yml',
    MINIMAL.replace(
      '      - name: alpha gate\n',
      '      - name: mystery step\n        uses: some-vendor/mystery-action@v1\n      - name: alpha gate\n'
    )
  );
  const list = gates(['--list', '--workflow', file]).out;
  assert.ok(list.includes('mystery step'), 'the unknown action disappeared from --list');
  assert.ok(list.includes('UNKNOWN'), 'the unknown action was not flagged as unknown');
  const run = gates(['--workflow', file]);
  assert.ok(run.out.includes('UNKNOWN  alpha :: mystery step'), 'the run did not report it');
});

test('a macOS job is named unrunnable here, and says CI is the gate for it', () => {
  const file = scratch('mac.yml', MINIMAL.replace('ubuntu-latest', 'macos-14'));
  const list = gates(['--list', '--unrunnable', '--workflow', file]).out;
  assert.ok(/needs a macOS runner/.test(list));
  assert.ok(/PUSHING AND READING\n\s+THE CI RUN IS THE GATE/.test(list));
  const run = gates(['--workflow', file]);
  assert.equal(run.code, 3, 'incomplete coverage must not exit 0 — `gates && git push` reads it');
  assert.ok(!run.out.includes('alpha-ran'), 'a gate for another platform was executed');
  assert.ok(run.out.includes('PARTIAL'), 'a partial run must not read as full coverage');
  assert.ok(!run.out.includes('ALL GATES PASSED'));
});

test('PARTIAL exits 3, and only --allow-partial turns that back into 0', () => {
  // `scripts/gates && git push` is the real calling convention; 29-of-39 must
  // not hand that `&&` a green light. 3 is distinct from a red gate's 1 so a
  // caller can tell "something failed" from "I did not run them all".
  const file = scratch(
    'partial.yml',
    `${MINIMAL}  mac:
    runs-on: macos-14
    steps:
      - name: mac gate
        run: echo mac-ran
`
  );
  const strict = gates(['--workflow', file]);
  assert.equal(strict.code, 3, 'a run with unrunnable gates exited as if it were green');
  assert.ok(/EXIT 3 — not a green light/.test(strict.out), 'the exit code was not explained');

  const accepted = gates(['--allow-partial', '--workflow', file]);
  assert.equal(accepted.code, 0, '--allow-partial must let a caller accept partial coverage');
  assert.ok(/--allow-partial/.test(accepted.out), 'the opt-in was not stated in the output');
  assert.ok(!accepted.out.includes('ALL GATES PASSED'), 'opting in must not fake full coverage');

  // CONTROL: with the unrunnable job gone, the same run is genuinely complete
  // and exits 0 with no flag — so the 3 above is about coverage, not a constant.
  const whole = gates(['--workflow', scratch('whole.yml', MINIMAL)]);
  assert.equal(whole.code, 0);
  assert.ok(whole.out.includes('ALL GATES PASSED'));
});

test('a gate whose program is missing is skipped with a named reason, not run blind', () => {
  const file = scratch(
    'missing-tool.yml',
    MINIMAL.replace('        run: echo alpha-ran\n', '        run: definitely-not-a-real-binary-xyz --go\n')
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 3, 'a skipped gate is incomplete coverage, not a green run');
  assert.ok(r.out.includes('`definitely-not-a-real-binary-xyz` is not on PATH'));
  assert.ok(r.out.includes('SKIPPED'));
});

test('--job scopes the run, and an unknown job id is an error not a silent empty run', () => {
  const file = scratch(
    'two-jobs.yml',
    `${MINIMAL}  beta:
    name: Beta
    runs-on: ubuntu-latest
    steps:
      - name: beta gate
        run: echo beta-ran
`
  );
  const scoped = gates(['--job', 'beta', '--workflow', file]);
  assert.equal(scoped.code, 3, 'a scoped run left a whole job unrun; that is not a green light');
  assert.ok(scoped.out.includes('beta-ran'));
  assert.ok(!scoped.out.includes('alpha-ran'), '--job did not scope the run');

  const bad = gates(['--job', 'nope', '--workflow', file]);
  assert.equal(bad.code, 2, 'an unknown job id must be an error');
  assert.ok(bad.out.includes('no such job'));
});

test('a scoped run never reads as full coverage', () => {
  const file = scratch(
    'scope.yml',
    `${MINIMAL}  beta:
    runs-on: ubuntu-latest
    steps:
      - name: beta gate
        run: echo beta-ran
`
  );
  const scoped = gates(['--job', 'alpha', '--workflow', file]);
  assert.equal(scoped.code, 3);
  assert.ok(
    !scoped.out.includes('ALL GATES PASSED'),
    'a run that skipped a whole job claimed every gate passed'
  );
  assert.ok(/SCOPED to --job alpha/.test(scoped.out), 'the scope was not stated');
  assert.ok(/beta/.test(scoped.out), 'the job that was not run was not named');

  // Control: unscoped, the same workflow DOES get the full-coverage line and 0.
  const full = gates(['--workflow', file]);
  assert.equal(full.code, 0);
  assert.ok(full.out.includes('ALL GATES PASSED'));
});

test('a matrix job is expanded to one entry per combination', () => {
  const file = scratch(
    'matrix.yml',
    `name: scratch
on: [push]
jobs:
  m:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        base: [one, two]
    steps:
      - name: build \${{ matrix.base }}
        run: echo built-\${{ matrix.base }}
`
  );
  const out = gates(['--list', '--workflow', file]).out;
  assert.ok(out.includes('m[base=one]') && out.includes('m[base=two]'), 'matrix was not expanded');
  assert.ok(out.includes('echo built-one') && out.includes('echo built-two'), 'matrix not substituted');
});

// ---------------------------------------------------------------------------
// The derived traps. Each is a question asked of the workflow, so each must
// change its answer when the workflow changes — both directions.
// ---------------------------------------------------------------------------

test('the "no fmt gate for src-tauri" trap is derived, and fires when CI drops the gate', () => {
  const original = fs.readFileSync(CI, 'utf8');

  // CI gained `cargo fmt --manifest-path src-tauri/Cargo.toml --check`, so the
  // real workflow must NOT report the trap any more. That direction is the one
  // that would go stale if the trap were remembered rather than derived.
  assert.ok(
    !/NO fmt gate covers src-tauri\/Cargo\.toml/.test(gates(['--list']).out),
    'the trap fires against a workflow that DOES cover src-tauri — it is remembered, not derived'
  );

  // CONTROL: take the gate away again and the trap must come back. Without this
  // the assertion above passes just as happily on a broken trap.
  // NOTE: `--all`. Two branches independently added this gate on 2026-09-07 and the
  // merge kept the `--all` form — src-tauri is a single-package workspace today, so the
  // two are equivalent, but the bare form would silently stop covering a member if one
  // were ever added. If the gate's wording moves again, this control must move with it;
  // the assertion below says so out loud rather than failing cryptically.
  const gate = '      - name: cargo fmt --check src-tauri\n' +
    '        run: cargo fmt --manifest-path src-tauri/Cargo.toml --all --check\n';
  assert.ok(original.includes(gate), 'the src-tauri fmt gate moved; update this test');
  const stripped = original.replace(gate, '');
  const out = gates(['--list', '--workflow', scratch('fmt-dropped.yml', stripped)]).out;
  assert.ok(
    /NO fmt gate covers src-tauri\/Cargo\.toml/.test(out),
    'control failed: dropping the fmt gate did not bring the trap back'
  );
});

test('the src-tauri workspace trap escalates when no CI step names the manifest', () => {
  const original = fs.readFileSync(CI, 'utf8');
  // Strip every reference to the manifest — the `run:` steps AND the cargo-deny
  // action's `manifest-path:` input, which the script translates into the same
  // flag.
  const stripped = original
    .split('\n')
    .filter((l) => !l.includes('src-tauri/Cargo.toml'))
    .join('\n');
  const out = gates(['--list', '--workflow', scratch('no-tauri.yml', stripped)]).out;
  assert.ok(
    /NO CI step names it. Everything in it is ungated/.test(out),
    'a workflow with no src-tauri coverage was not called out'
  );
  // Control: the real workflow DOES cover it, so it must not say that.
  assert.ok(!/Everything in it is ungated/.test(gates(['--list']).out));
});

test('the real workflow reports the src-tauri feature requirement with the traps', () => {
  const out = gates(['--list']).out;
  assert.ok(/EXCLUDES src-tauri\/Cargo\.toml/.test(out));
  assert.ok(/--features radio/.test(out), 'the radio feature requirement was not surfaced');
});
