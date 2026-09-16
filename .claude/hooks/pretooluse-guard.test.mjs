// Regression suite for .claude/hooks/pretooluse-guard.mjs.
//
// EVERY rule is tested in BOTH directions, and that is the point of the file rather than a
// nicety. This project's own doctrine: "a guard must be shown both to fire and not to fire
// — one direction is half a test." A guard shown only firing is indistinguishable from a
// guard that refuses everything, and a guard that refuses ordinary work gets switched off
// within a day, after which it protects nothing at all. So each MUST-BLOCK case below is
// paired with the nearest legitimate command that MUST pass.
//
// The shared-checkout detection is exercised against REAL git state, not a stub: the suite
// builds a repo with a linked worktree (shared — rules 1-3 arm) and a second, independent
// `git init` scratch repo (not shared — they stand down). Stubbing that out would test the
// wiring and not the mechanism.
//
// Run: node --test .claude/hooks/pretooluse-guard.test.mjs

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync, spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Block without spinning the CPU and without spawning a process per wait; these tests wait
// on real process transitions and are called in loops.
const SLEEPER = new Int32Array(new SharedArrayBuffer(4));
const sleep = (ms) => Atomics.wait(SLEEPER, 0, 0, ms);

const HERE = path.dirname(fileURLToPath(import.meta.url));
const HOOK = path.join(HERE, 'pretooluse-guard.mjs');

// --- a real shared checkout, and a real scratch repo ------------------------
const LAB = fs.mkdtempSync(path.join(os.tmpdir(), 'nexus-guard-test-'));
process.on('exit', () => fs.rmSync(LAB, { recursive: true, force: true }));

const git = (...args) => {
  const r = spawnSync('git', args, { encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout;
};

const SHARED = path.join(LAB, 'shared');
const LINKED = path.join(LAB, 'linked');
const SCRATCH = path.join(LAB, 'scratch');

fs.mkdirSync(SHARED);
git('init', '-q', SHARED);
git('-C', SHARED, 'config', 'user.name', 'KD9TAW');
git('-C', SHARED, 'config', 'user.email', 'kd9taw@protonmail.com');
fs.writeFileSync(path.join(SHARED, 'README.md'), 'hi\n');
git('-C', SHARED, 'add', 'README.md');
git('-C', SHARED, 'commit', '-qm', 'init');
git('-C', SHARED, 'worktree', 'add', '-q', '-b', 'sibling', LINKED);

fs.mkdirSync(SCRATCH);
git('init', '-q', SCRATCH);

// --- driver -----------------------------------------------------------------
// Feeds the REAL PreToolUse payload shape over stdin, so the tests exercise the
// interface the harness uses rather than an exported function.
function run(command, { cwd = SHARED, tool = 'Bash', filePath } = {}) {
  const payload = JSON.stringify({
    session_id: 'test', hook_event_name: 'PreToolUse', cwd,
    tool_name: tool,
    tool_input: filePath ? { file_path: filePath, old_string: 'a', new_string: 'b' } : { command },
  });
  const r = spawnSync(process.execPath, [HOOK], { input: payload, encoding: 'utf8' });
  let decision = 'allow';
  if (r.status === 2) decision = 'block';
  else if (r.stdout.trim()) {
    try { decision = JSON.parse(r.stdout).hookSpecificOutput.permissionDecision; } catch { /* allow */ }
  }
  return { decision, text: `${r.stdout}${r.stderr}`, status: r.status };
}

const blocks = (name, command, opts) => test(`BLOCK  ${name}`, () => {
  const r = run(command, opts);
  assert.equal(r.decision, 'block', `expected a refusal for: ${command}\n${r.text}`);
});
const asks = (name, command, opts) => test(`ASK    ${name}`, () => {
  const r = run(command, opts);
  assert.equal(r.decision, 'ask', `expected a confirmation for: ${command}\n${r.text}`);
});
const allows = (name, command, opts) => test(`ALLOW  ${name}`, () => {
  const r = run(command, opts);
  assert.equal(r.decision, 'allow', `expected silence for: ${command}\n${r.text}`);
});

// ===========================================================================
// Rule 1 — working-tree-wide destructive git ops in a shared checkout.
// ===========================================================================
blocks('reset --hard', `git -C ${SHARED} reset --hard HEAD`);
blocks('checkout .', `git -C ${SHARED} checkout .`);
blocks('restore . (worktree)', `git -C ${SHARED} restore .`);
blocks('clean -fd', `git -C ${SHARED} clean -fd`);
blocks('bare stash', `git -C ${SHARED} stash`);
blocks('stash pop', `git -C ${SHARED} stash pop`);
blocks('stash clear', `git -C ${SHARED} stash clear`);
blocks('checkout --force onto a branch', `git -C ${SHARED} checkout -f sibling`);

allows('reset --soft HEAD~1 is the sanctioned undo', `git -C ${SHARED} reset --soft HEAD~1`);
allows('reset with no --hard leaves the tree alone', `git -C ${SHARED} reset HEAD~1`);
allows('restore --staged . only unstages', `git -C ${SHARED} restore --staged .`);
allows('restore of a named file', `git -C ${SHARED} restore -- crates/js8/src/lib.rs`);
allows('clean --dry-run', `git -C ${SHARED} clean -nd`);
allows('stash list is read-only', `git -C ${SHARED} stash list --format='%H %gs'`);
allows('stash push -u -m <tag> is the sanctioned form', `git -C ${SHARED} stash push -u -m "guard-work"`);
allows('stash apply <sha>', `git -C ${SHARED} stash apply deadbeef`);
allows('checkout of a branch', `git -C ${SHARED} checkout -b feat/x origin/main`);
allows('clean -fd in a throwaway repo nobody shares', `git -C ${SCRATCH} clean -fd`);
allows('the named override, spelled in the command', `NEXUS_ALLOW_DESTRUCTIVE=1 git -C ${SHARED} clean -fd`);

// ===========================================================================
// Rule 2 — broad staging.
// ===========================================================================
blocks('add -A', `git -C ${SHARED} add -A`);
blocks('add .', `git -C ${SHARED} add .`);
blocks('add --all', `git -C ${SHARED} add --all`);
blocks('add -u sweeps every tracked modification', `git -C ${SHARED} add -u`);
blocks('commit -am', `git -C ${SHARED} commit -am "wip"`);
blocks('commit --all', `git -C ${SHARED} commit --all -m "wip"`);

allows('add by explicit path', `git -C ${SHARED} add crates/js8/src/lib.rs src/App.tsx`);
allows('add -A scoped to a pathspec', `git -C ${SHARED} add -A -- crates/js8`);
allows('add -p chooses hunks', `git -C ${SHARED} add -p`);
allows('commit --amend is not commit --all', `git -C ${SHARED} commit --amend --no-edit`);
allows('commit --allow-empty is not commit --all', `git -C ${SHARED} commit --allow-empty -m "trigger ci"`);
allows('commit by explicit path', `git -C ${SHARED} commit -- crates/js8/src/lib.rs -m "fix"`);
allows('add -A inside a fresh git init scratch dir', `cd ${SCRATCH} && git add -A`);
allows('the named override', `NEXUS_ALLOW_BROAD_ADD=1 git -C ${SHARED} add -A`);
// The file that documents these commands must not be refused for containing them.
allows('a heredoc body quoting a forbidden command',
  `cat > notes.md <<'EOF'\nNever run git add -A here.\nEOF`);
allows('a forbidden command inside a quoted string', `echo "never run git add -A"`);

// ===========================================================================
// Rule 3 — a mutating git with no -C <absolute path>.
// ===========================================================================
asks('commit with no -C', 'git commit -m "fix: thing"');
asks('push with no -C', 'git push origin HEAD');
asks('merge with no -C', 'git merge origin/main');

allows('read-only git needs no confirmation', 'git status --short');
allows('log is read-only', 'git log --oneline -20');
allows('an in-line cd to an absolute path is unambiguous', `cd ${SHARED} && git commit -m "fix"`);
allows('a mutating git in a scratch repo', 'git commit -m "wip"', { cwd: SCRATCH });
allows('the named override', 'NEXUS_ALLOW_GIT_CWD=1 git commit -m "fix"');

// ===========================================================================
// Rule 4 — a piped gate. The one that shipped a faked green in 1.10.3.
// ===========================================================================
blocks('cargo test | grep', 'cargo test --workspace | grep -c "test result"');
blocks('cargo test 2>&1 | tail', 'cargo test --workspace 2>&1 | tail -30');
blocks('cargo clippy | head', 'cargo clippy --all-targets | head -50');
blocks('npm run test:native | grep', 'npm run test:native | grep FAIL');
blocks('scripts/gates | head', './scripts/gates | head -40');
blocks('node --test | tail', 'node --test scripts/gates.test.mjs | tail -5');

// `tee` — the entry the enumerated list was short, found in the wild on 2026-09-15 when a
// cross-compile piped into it exited 77 and was reported as 0.
blocks('the 1.10.3 shape in its 2026-09-15 form: a build into tee',
  'cargo build --release --target x86_64-pc-windows-gnu | tee build.log');
blocks('scripts/gates | tee', './scripts/gates | tee gates.log');
blocks('npm run test:native | tee', 'npm run test:native | tee out.log');
// The class, not the list: a tail that was never enumerated must be refused too. If this
// passes while the tee cases block, the rule is still a list and still one entry short.
blocks('a gate into a tail no list would have held', 'cargo test --workspace | md5sum');
blocks('a gate into a tail no list would have held (2)', 'cargo clippy --all-targets | xxd');
blocks('a gate into tee then grep', 'cargo test --workspace | tee run.log | grep FAIL');

// The taught fix must actually be accepted, or the message sends people into a refusal.
allows('set -o pipefail keeps the build AND the tee', 'set -o pipefail; cargo build --release | tee build.log');
allows('set -euo pipefail is the same promise', 'set -euo pipefail; cargo test --workspace | tee run.log');
allows('reading PIPESTATUS keeps the status', 'cargo test --workspace | tee run.log; echo "exit=${PIPESTATUS[0]}"');

allows('grep on a log file is not a gate', 'grep -n "FAIL" guardhooks.log');
allows('grep on a log file, piped onward to tee', 'grep -n "FAIL" build.log | tee found.txt');
allows('cat into tee is not a gate', 'cat run.log | tee copy.log');
allows('cat into grep is not a gate', 'cat guardhooks.log | grep FAIL');
allows('cargo metadata is a query, not a gate', 'cargo metadata --format-version 1 | jq ".packages | length"');
allows('git log into head is not a gate', 'git log --oneline | head -5');
allows('ls into head', 'ls -la crates | head');
allows('a gate redirected to a file keeps its exit code', 'cargo test --workspace > guardhooks.log 2>&1');
allows('pipefail preserves the gate status', 'set -o pipefail; cargo test --workspace | tail -30');
allows('the named override', 'NEXUS_ALLOW_PIPED_GATE=1 cargo test --workspace | tail -30');

// ===========================================================================
// Rule 5 — scripts/gates --allow-partial outside remote-staging.yml.
// ===========================================================================
blocks('--allow-partial', './scripts/gates --allow-partial');
blocks('--allow-partial with a job scope', 'scripts/gates --job ui,test --allow-partial');

allows('a scoped run names what it skipped', 'scripts/gates --job ui,test');
allows('listing what cannot run here', 'scripts/gates --list --unrunnable');
allows('a plain full run', './scripts/gates');
allows('the named override', 'NEXUS_ALLOW_PARTIAL_GATES=1 ./scripts/gates --allow-partial');

// ===========================================================================
// Rule 6 — mutating a worktree with a verification in flight.
//
// Tested against a REAL process with a REAL cwd, observed through /proc exactly as the
// hook observes it. The negative direction matters more than the positive one here: the
// failure that would sink this rule is arming when nothing is running, so the same command
// is asserted to ASK with the run up and to pass once it has exited.
// ===========================================================================

// A stand-in gate: a real executable at a real scripts/gates path, carrying the SAME
// `#!/usr/bin/env node` shebang the genuine article has, so its /proc cmdline is the real
// shape — `node /…/scripts/gates`, not `scripts/gates`. That difference is not cosmetic:
// the first version of the hook matched argv[0] only and therefore could not see the real
// gate runner at all. These three cases failed and that is how it was found.
const stubDir = path.join(SHARED, 'scripts');
fs.mkdirSync(stubDir, { recursive: true });
const IDLE = '#!/usr/bin/env node\nsetTimeout(() => {}, 120000);\n';
const GATE_STUB = path.join(stubDir, 'gates');
fs.writeFileSync(GATE_STUB, IDLE);
fs.chmodSync(GATE_STUB, 0o755);
// A long-running process that is NOT a verification — the control for "any old process".
const SERVER_STUB = path.join(stubDir, 'devserver');
fs.writeFileSync(SERVER_STUB, IDLE);
fs.chmodSync(SERVER_STUB, 0o755);

const spawned = [];
function startInFlight(bin, cwd) {
  const child = spawn(bin, [], { cwd, stdio: 'ignore' });
  spawned.push(child);
  // Wait for the kernel to publish cwd/cmdline before probing.
  for (let i = 0; i < 40; i++) {
    try { if (fs.readlinkSync(`/proc/${child.pid}/cwd`)) return child; } catch { /* not up yet */ }
    sleep(50);
  }
  return child;
}
function waitGone(pid) {
  for (let i = 0; i < 40 && fs.existsSync(`/proc/${pid}`); i++) sleep(50);
}
process.on('exit', () => spawned.forEach((c) => { try { c.kill('SIGKILL'); } catch { /* gone */ } }));

const linuxOnly = { skip: fs.existsSync('/proc/uptime') ? false : 'rule 6 is /proc-based; no /proc here' };

test('ASK    a merge into a worktree with a gate in flight', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    const r = run(`git -C ${SHARED} merge origin/main`);
    assert.equal(r.decision, 'ask', r.text);
    assert.match(r.text, /IN FLIGHT/);
    assert.match(r.text, new RegExp(`pid ${child.pid}`));
    assert.match(r.text, /running \d+s/);           // says how long
    assert.match(r.text, /NEXUS_ALLOW_MUTATE_DURING_RUN=1/);
  } finally { child.kill('SIGKILL'); }
});

test('ASK    an Edit to a file under a running gate', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    const r = run(null, { tool: 'Edit', filePath: path.join(SHARED, 'README.md') });
    assert.equal(r.decision, 'ask', r.text);
    assert.match(r.text, /IN FLIGHT/);
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  the SAME merge once the run has exited (the control that matters)', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  const during = run(`git -C ${SHARED} merge origin/main`);
  assert.equal(during.decision, 'ask', 'positive control: must arm while it is up');
  child.kill('SIGKILL');
  waitGone(child.pid);
  const after = run(`git -C ${SHARED} merge origin/main`);
  assert.equal(after.decision, 'allow', `must stand down once nothing is running\n${after.text}`);
});

test('ALLOW  a merge into a DIFFERENT worktree from the one being verified', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    assert.equal(run(`git -C ${LINKED} merge origin/main`).decision, 'allow');
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  a long-running process that is not a verification', linuxOnly, () => {
  const child = startInFlight(SERVER_STUB, SHARED);
  try {
    assert.equal(run(`git -C ${SHARED} merge origin/main`).decision, 'allow');
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  read-only git during a run', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    assert.equal(run(`git -C ${SHARED} status --short`).decision, 'allow');
    assert.equal(run(`git -C ${SHARED} log --oneline -5`).decision, 'allow');
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  an Edit outside the tree being verified', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    const r = run(null, { tool: 'Edit', filePath: path.join(LINKED, 'README.md') });
    assert.equal(r.decision, 'allow', r.text);
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  the named override during a run', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    const r = run(`NEXUS_ALLOW_MUTATE_DURING_RUN=1 git -C ${SHARED} merge origin/main`);
    assert.equal(r.decision, 'allow', r.text);
  } finally { child.kill('SIGKILL'); }
});

test('ALLOW  commit and add are not tree mutations, even mid-run', linuxOnly, () => {
  const child = startInFlight(GATE_STUB, SHARED);
  try {
    assert.equal(run(`git -C ${SHARED} commit -- README.md -m "fix"`).decision, 'allow');
    assert.equal(run(`git -C ${SHARED} add README.md`).decision, 'allow');
  } finally { child.kill('SIGKILL'); }
});

// ===========================================================================
// Harness contract.
// ===========================================================================
allows('a non-Bash tool is not this hook\'s business', `git -C ${SHARED} add -A`, { tool: 'Read' });

test('MISC   a refusal names the rule, the reason and the safer command', () => {
  const r = run(`git -C ${SHARED} add -A`);
  assert.equal(r.decision, 'block');
  assert.match(r.text, /REFUSED/);
  assert.match(r.text, /why: /);
  assert.match(r.text, /instead: /);
  assert.match(r.text, /NEXUS_ALLOW_BROAD_ADD=1/);
});

test('MISC   a malformed payload asks rather than blocking or waving through', () => {
  const r = spawnSync(process.execPath, [HOOK], { input: '{ not json', encoding: 'utf8' });
  assert.equal(r.status, 0, 'a guard that crashed into blocking every Bash call would brick a session');
  assert.match(r.stdout, /"permissionDecision":"ask"/);
  assert.match(r.stdout, /NOT vouching/);
});

// A control for the control: the shared/scratch distinction the whole precision story
// rests on must be REAL. If SCRATCH were somehow seen as shared, every "allowed in a
// scratch dir" case above would pass for the wrong reason.
test('MISC   positive control: the same command blocks in shared and passes in scratch', () => {
  assert.equal(run(`git -C ${SHARED} clean -fd`).decision, 'block');
  assert.equal(run(`git -C ${SCRATCH} clean -fd`).decision, 'allow');
});
