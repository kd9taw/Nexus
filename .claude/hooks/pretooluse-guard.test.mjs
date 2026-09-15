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
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

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
function run(command, { cwd = SHARED, tool = 'Bash' } = {}) {
  const payload = JSON.stringify({
    session_id: 'test', hook_event_name: 'PreToolUse', cwd,
    tool_name: tool, tool_input: { command },
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

allows('grep on a log file is not a gate', 'grep -n "FAIL" guardhooks.log');
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
