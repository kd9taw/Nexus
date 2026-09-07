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

  // And it is counted as a gate, not swallowed as setup. (Anchor on the summary
  // line, not the first "N gate(s)" in the output — the per-job banners use the
  // same words.)
  const n = (s) => Number(s.match(/(\d+) gate\(s\); \d+ cannot run here/)[1]);
  assert.equal(n(after.out), n(before.out) + 1, 'the gate count did not go up by exactly one');
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
  assert.equal(run.code, 0, 'skipping an unrunnable job is not a failure');
  assert.ok(!run.out.includes('alpha-ran'), 'a gate for another platform was executed');
  assert.ok(run.out.includes('PARTIAL'), 'a partial run must not read as full coverage');
  assert.ok(!run.out.includes('ALL GATES PASSED'));
});

test('a gate whose program is missing is skipped with a named reason, not run blind', () => {
  const file = scratch(
    'missing-tool.yml',
    MINIMAL.replace('        run: echo alpha-ran\n', '        run: definitely-not-a-real-binary-xyz --go\n')
  );
  const r = gates(['--workflow', file]);
  assert.equal(r.code, 0);
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
  assert.equal(scoped.code, 0);
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
  assert.equal(scoped.code, 0);
  assert.ok(
    !scoped.out.includes('ALL GATES PASSED'),
    'a run that skipped a whole job claimed every gate passed'
  );
  assert.ok(/SCOPED to --job alpha/.test(scoped.out), 'the scope was not stated');
  assert.ok(/beta/.test(scoped.out), 'the job that was not run was not named');

  // Control: unscoped, the same workflow DOES get the full-coverage line.
  assert.ok(gates(['--workflow', file]).out.includes('ALL GATES PASSED'));
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

test('the "no fmt gate for src-tauri" trap is derived, and clears when CI adds one', () => {
  const original = fs.readFileSync(CI, 'utf8');

  const before = gates(['--list']).out;
  assert.ok(
    /NO fmt gate covers src-tauri\/Cargo\.toml/.test(before),
    'control failed: the trap is not reported against the real workflow'
  );

  const fixed = original.replace(
    '      - name: cargo fmt --check\n        run: cargo fmt --all --check\n',
    '      - name: cargo fmt --check\n        run: cargo fmt --all --check\n' +
      '      - name: fmt src-tauri\n' +
      '        run: cargo fmt --manifest-path src-tauri/Cargo.toml --check\n'
  );
  assert.notEqual(fixed, original, 'the fmt step moved; update this test');
  const after = gates(['--list', '--workflow', scratch('fmt-fixed.yml', fixed)]).out;
  assert.ok(
    !/NO fmt gate covers src-tauri\/Cargo\.toml/.test(after),
    'the trap survived a workflow that fixed it — it is remembered, not derived'
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
