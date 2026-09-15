#!/usr/bin/env node
// Nexus PreToolUse guard — the Claude-side sibling of .githooks/pre-push.
//
// WHY THIS EXISTS. This project's doctrine says it plainly: a tool whose only trigger is
// somebody remembering is not a gate. A 2026-09-15 audit of ~71 accumulated lesson files
// found 14 that record a rule which ALREADY EXISTED and was violated anyway — and found
// that .githooks/pre-push, the only rule here that is both always-loaded and mechanised,
// is the only hard rule that has not recurred. Prose has a measured failure rate in this
// repo. Mechanism does not. So the six shapes below, each of which has already destroyed
// work or faked a result here, stop being advice and start being a refusal. The sixth was
// not an agent's mistake but the maintainer's, which is the best argument for all six:
// knowing a rule is not what protects you from breaking it.
//
// It is NOT a git hook — git will never run it. It hangs off .claude/hooks/ because that
// is where a Claude Code hook belongs and because .claude/ is already tracked here, so
// every worktree gets it with no per-clone step. Enabling it is a settings.json edit the
// MAINTAINER makes; see "ENABLING" at the bottom of this header. Shipping the script is
// deliberately not the same act as switching it on.
//
// -- THE SIX, and why each is block or ask --------------------------------------------
//
//  1. WORKING-TREE-WIDE DESTRUCTIVE GIT OPS in a shared checkout — reset --hard,
//     checkout . / restore ., clean -f, bare git stash / stash pop / stash clear.  BLOCK.
//     Many agents share one checkout. These wipe every concurrent agent's uncommitted
//     work, and uncommitted work leaves NO GIT OBJECT: no reflog entry, nothing to
//     recover, no undo. `git stash` is the trap — it reports success while pocketing
//     everyone else's changes, so the loss is found hours later by whoever lost it.
//     Block, not ask, because the damage is unrecoverable and the safe substitute is
//     always available (reset --soft HEAD~1; a throwaway clone).
//                                              Override: NEXUS_ALLOW_DESTRUCTIVE=1
//
//  2. BROAD STAGING — git add -A / add . / add -u / commit -a.               BLOCK.
//     The staging area is shared state. Between one agent's `add` and its `commit`,
//     another agent's broad add sweeps unrelated files into a commit whose message
//     describes none of them. Silent: both sets of gates pass and nothing looks wrong
//     until someone reads the log. Block, not ask, because staging by explicit path is
//     never harder and the failure has no symptom to notice later.
//                                              Override: NEXUS_ALLOW_BROAD_ADD=1
//
//  3. A MUTATING `git` WITH NO -C <abs path> in a shared checkout.            ASK.
//     A shell cwd persists between calls, so the edit lands in the right tree while the
//     commit runs in the wrong one; a branch name resolves from any worktree, so a push
//     "succeeds" carrying a tip that lacks the fix. ASK rather than BLOCK for one reason:
//     this is the highest-frequency shape of the six by an order of magnitude, and a
//     guard that interrupts ordinary work gets switched off within a day and then
//     protects nothing. Two precision cuts keep the noise survivable — only MUTATING
//     subcommands are considered (a wrong-tree `git status` gives a wrong answer, but it
//     does not write anything), and an in-line `cd /abs/path && git ...` is accepted
//     because that command line is not ambiguous about which tree it means.
//                                              Override: NEXUS_ALLOW_GIT_CWD=1
//
//  4. A PIPED GATE — a test/lint/build piped into grep/tail/head/wc/...      BLOCK.
//     Without `set -o pipefail`, a pipeline's status is the RIGHT-hand side's, so the
//     gate's own exit code is discarded and a red run reads green. This one is not
//     hypothetical: it SHIPPED a faked-green gate in release 1.10.3, and the rule had
//     been written down four separate times before that happened. Block, because the
//     whole failure mode is that the result LOOKS fine — there is no later moment at
//     which anyone catches it.
//                                              Override: NEXUS_ALLOW_PIPED_GATE=1
//
//  5. `scripts/gates --allow-partial`.                                        BLOCK.
//     It converts "coverage was incomplete" into exit 0. A control run showed a missing
//     binary exiting 0 with the deploy proceeding. Its ONE legitimate site is inside
//     .github/workflows/remote-staging.yml, which is a YAML file — never a Bash tool
//     call — so this guard cannot false-positive on the sanctioned use. Block.
//                                              Override: NEXUS_ALLOW_PARTIAL_GATES=1
//
//  6. MUTATING A WORKTREE THAT HAS A VERIFICATION IN FLIGHT.                  ASK.
//     A git merge / rebase / checkout / cherry-pick, or an Edit or Write to a file, while
//     scripts/gates, cargo test, vitest or npm test is running with that tree as its cwd.
//     The run then reports on a tree that no longer exists, and the reds it invents are
//     indistinguishable from real ones. On 2026-09-15 a merge into a worktree mid-run
//     produced five reds, four of them manufactured, and the result was briefly believed
//     and reported as a verdict — a whole gate cycle wasted and, worse, a wrong answer
//     trusted. ASK, not BLOCK, for two honest reasons: a merge into a tree whose run has
//     already finished-but-not-been-read is legitimate and common, and the operator may
//     genuinely mean to abandon the run. The prompt names WHAT is running, its pid and
//     HOW LONG it has been going, so the answer can be informed rather than reflexive.
//     This is the only rule that reaches the Edit/Write tools, and the only one not gated
//     on a shared checkout — a run in flight is just as clobberable in a solo tree.
//                                              Override: NEXUS_ALLOW_MUTATE_DURING_RUN=1
//                                              (env-only for Edit/Write — see below)
//
// -- HOW AN OVERRIDE IS SPELLED --------------------------------------------------------
//
// Same shape as NEXUS_RELEASE_APPROVED=1 on the pre-push gate: a named, visible, per-
// command assignment that leaves a trace in the transcript.
//
//     NEXUS_ALLOW_DESTRUCTIVE=1 git -C /tmp/throwaway clean -fd
//
// The assignment is read out of the COMMAND TEXT, not the environment, because the Bash
// tool does not persist env between calls — so `export` in an earlier call could never
// reach here, and an override that silently stopped working would be worse than none. A
// variable genuinely set in the hook's own process environment is honoured too, for a
// maintainer who means to stand an exception up for a whole session. There is no silent
// bypass and no --no-verify equivalent: the override names itself in the command you ran.
//
// ONE EXCEPTION, and it is a real limitation rather than a design: an Edit or Write has no
// command line to carry a prefix, so rule 6's override on those tools can only come from
// the environment Claude Code itself was started with. Answering "yes" to the prompt is
// the practical way through; the env var is for standing the exception up deliberately.
//
// -- WHAT KEEPS IT FROM CRYING WOLF ----------------------------------------------------
//
// False positives are the failure mode that gets a guard disabled, so rules 1-3 fire ONLY
// inside a SHARED CHECKOUT — a working tree whose git common dir has at least one linked
// worktree registered. A fresh `git init` scratch dir has none. A throwaway CLONE has none
// (clones do not share a common dir; only worktrees do). So `git add -A` in a sandbox and
// `git clean -fd` in a disposable clone are allowed with no override, which is correct:
// there is no second agent there to rob. Set NEXUS_GUARD_ROOTS to a colon-separated list
// of absolute prefixes to force-protect paths that test otherwise.
//
// Other deliberate near-misses, each covered by a test that MUST stay green:
//   - `git stash list|show|apply <sha>` and `git stash push -u -m "<tag>"` are the
//     sanctioned forms and pass; bare `stash`, `pop` and `clear` do not.
//   - `git restore --staged .` unstages without touching any working tree, and passes;
//     adding --worktree does not.
//   - `git reset --soft HEAD~1` and `git reset HEAD~1` pass. Only --hard is refused.
//   - `git add -A -- crates/js8` is scoped to a path and passes.
//   - `git commit --amend` and `--allow-empty` both start with "--am"/"--all" and must
//     never be read as `--all`; exact token matching, not prefix matching.
//   - `grep` on a log file, `cargo metadata | jq`, `git log | head` are not piped gates —
//     rule 4 needs a GATE on the left of the pipe, by name, not any command at all.
//   - A pipeline preceded by `set -o pipefail` keeps the gate's status and passes.
//   - Heredoc bodies are skipped entirely, so writing documentation that CONTAINS
//     `git add -A` does not trip rule 2. (This file is that document.)
//
//   - Rule 6 stands down completely when nothing is running, which is the normal case and
//     costs one /proc scan with no subprocess. `git status` during a run passes; so does a
//     merge into a DIFFERENT worktree from the one being verified; so does a long-running
//     dev server, which is not a verification.
//
// -- KNOWN GAPS, written down rather than papered over ---------------------------------
//   - Rule 6 is Linux-only (/proc), never arms on macOS, and misses a runner that chdir'd
//     away or was started from a parent directory. It also does not see a tree mutation
//     made by an ordinary shell command (`rm -rf crates/x`, `sed -i`) — only git and the
//     Edit/Write tools. Every one of those fails OPEN. See the block above rule 6's code.
//   - Only the Bash tool is inspected, apart from rule 6's reach into Edit/Write. A
//     destructive op reached some other way — an MCP
//     server, a script the agent writes and then runs, `bash -c "$(...)"` — is invisible
//     here. Rule 4's real siblings are the same: a gate run from inside a shell script is
//     not seen. This guard covers the shape agents actually type, not every shape.
//   - `cargo test | tee run.log` also discards the gate's status, and is NOT blocked. tee
//     is left out of the discard list because it keeps the full output for review and
//     blocking it would hit a common honest pattern. Redirect instead: `cargo test
//     > run.log 2>&1` keeps the exit code.
//   - Rule 3 accepts `cd /abs && git commit`. That is genuinely unambiguous for that one
//     command line; it does not stop the NEXT call inheriting a cwd nobody meant.
//   - The shared-checkout test costs one `git rev-parse` per matching command. It runs
//     only after a dangerous shape has already matched, so ordinary commands pay nothing.
//   - A gate list is a list, and lists rot (see scripts/gates, which refuses to keep one).
//     Rule 4's GATE_* names below are a transcription and will lag a new tool. It fails
//     open for anything not named — a missed piped gate, never a blocked innocent one.
//
// -- CONTRACT WITH THE HARNESS ---------------------------------------------------------
// stdin: the PreToolUse JSON payload. BLOCK is emitted as exit 2 with the reason on
// stderr, which every Claude Code version understands as a refusal — deliberately not the
// JSON form, because a harness that did not understand the JSON would fail OPEN and a
// gate that fails open is the thing this project keeps getting burned by. ASK needs the
// JSON form (exit codes cannot express it) and therefore degrades to "allowed" on a
// harness that does not support it; that is acceptable for the warn tier and not for the
// block tier, which is exactly why they are emitted differently.
// An internal error in this file emits ASK, never BLOCK and never silence: a guard that
// crashed into blocking every Bash call would brick a session, and one that crashed into
// silence would be the failure it was written to prevent.
//
// -- ENABLING (the maintainer's call, not the hook's) ----------------------------------
// Add to ~/.claude/settings.json (user-level, this machine) or .claude/settings.json
// (repo-local, shared with anyone who clones). The matcher must name the edit tools as
// well as Bash, or rule 6 sees merges but not the Edit that does the same damage:
//
//   { "hooks": { "PreToolUse": [ {
//       "matcher": "Bash|Edit|Write|MultiEdit|NotebookEdit",
//       "hooks": [ { "type": "command",
//                    "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/pretooluse-guard.mjs" } ]
//   } ] } }
//
// Tests: node --test .claude/hooks/pretooluse-guard.test.mjs

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

// ---------------------------------------------------------------------------
// Tokenizer. Small on purpose, but it has to get three things right or the
// guard misreads real commands: quoting, operator splitting, and heredocs.
// ---------------------------------------------------------------------------

const OPERATORS = ['&&', '||', ';;', ';', '|&', '|', '&', '\n', '(', ')', '{', '}'];
// Separators that end a pipeline. '|' does NOT — it joins stages within one.
const PIPELINE_BREAKS = new Set(['&&', '||', ';;', ';', '&', '\n', '(', ')', '{', '}']);

function tokenize(src) {
  const out = [];
  let i = 0;
  let expectTarget = false; // the word after `>` is a filename, not an argument
  const n = src.length;
  while (i < n) {
    const c = src[i];

    // Whitespace (newline is an operator, handled below).
    if (c === ' ' || c === '\t' || c === '\r') { i++; continue; }

    // Comment to end of line.
    if (c === '#' && (out.length === 0 || out[out.length - 1].op)) {
      while (i < n && src[i] !== '\n') i++;
      continue;
    }

    // Heredoc: skip the body wholesale. Documentation that quotes a forbidden
    // command must not trip the guard that forbids it.
    if (c === '<' && src[i + 1] === '<' && src[i + 2] !== '<') {
      i += 2;
      if (src[i] === '-') i++;
      while (i < n && (src[i] === ' ' || src[i] === '\t')) i++;
      let delim = '';
      if (src[i] === "'" || src[i] === '"') {
        const q = src[i++];
        while (i < n && src[i] !== q) delim += src[i++];
        i++;
      } else {
        while (i < n && /[A-Za-z0-9_]/.test(src[i])) delim += src[i++];
      }
      // Advance past the rest of this line, then past body lines until the delimiter.
      const nl = src.indexOf('\n', i);
      if (nl === -1 || !delim) { i = n; continue; }
      // Emit the remainder of the current line as ordinary tokens by rewinding to it,
      // then resuming after the body. Simpler and safe: re-tokenize that slice.
      for (const t of tokenize(src.slice(i, nl))) out.push(t);
      let j = nl + 1;
      while (j < n) {
        const end = src.indexOf('\n', j);
        const line = src.slice(j, end === -1 ? n : end);
        j = end === -1 ? n : end + 1;
        if (line.trim() === delim) break;
      }
      out.push({ v: '\n', op: true });
      i = j;
      continue;
    }

    // Redirections, BEFORE the operator scan: `2>&1` and `&>x` both start with a
    // character that is otherwise a pipeline separator, and splitting a pipeline on the
    // `&` of `2>&1` would hide the commonest real form of rule 4 entirely.
    const rd = /^(&>>|&>|\d*>>|\d*>&\d*|\d*>|\d*<&\d*|\d*<)/.exec(src.slice(i));
    if (rd) {
      out.push({ v: rd[0], op: false, redir: true });
      i += rd[0].length;
      expectTarget = !/[&]\d*$/.test(rd[0]);
      continue;
    }

    // Operators.
    let matched = null;
    for (const op of OPERATORS) if (src.startsWith(op, i)) { matched = op; break; }
    if (matched) { out.push({ v: matched, op: true }); i += matched.length; continue; }

    // A word, possibly with embedded quoting.
    let word = '';
    let quoted = false;
    while (i < n) {
      const d = src[i];
      if (d === ' ' || d === '\t' || d === '\r' || d === '\n') break;
      if (OPERATORS.some((op) => src.startsWith(op, i))) break;
      if (d === "'") {
        quoted = true; i++;
        while (i < n && src[i] !== "'") word += src[i++];
        i++;
        continue;
      }
      if (d === '"') {
        quoted = true; i++;
        while (i < n && src[i] !== '"') {
          if (src[i] === '\\' && i + 1 < n) { i++; word += src[i++]; continue; }
          word += src[i++];
        }
        i++;
        continue;
      }
      if (d === '\\' && i + 1 < n) { i++; word += src[i++]; continue; }
      word += src[i++];
    }
    if (word !== '' || quoted) {
      out.push({ v: word, op: false, quoted, redirTarget: expectTarget });
      expectTarget = false;
    }
  }
  return out;
}

// Split a token stream into pipelines; split each pipeline into its stages.
// Returns [{ stages: [argv-token-array, ...] }, ...].
function pipelines(tokens) {
  const result = [];
  let stages = [[]];
  const flush = () => {
    const kept = stages.filter((s) => s.length);
    if (kept.length) result.push({ stages: kept });
    stages = [[]];
  };
  for (const t of tokens) {
    if (t.op && PIPELINE_BREAKS.has(t.v)) { flush(); continue; }
    if (t.op && (t.v === '|' || t.v === '|&')) { stages.push([]); continue; }
    if (t.op) continue;
    stages[stages.length - 1].push(t);
  }
  flush();
  return result;
}

// Strip leading VAR=value assignments and transparent wrappers, returning the real argv
// plus the assignments seen (which is how an override is spelled).
const WRAPPERS = new Set(['env', 'time', 'command', 'nohup', 'sudo', 'nice', 'ionice', 'exec']);
function argvOf(stage) {
  const assignments = [];
  let k = 0;
  for (;;) {
    while (k < stage.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(stage[k].v)) {
      assignments.push(stage[k].v); k++;
    }
    if (k < stage.length && WRAPPERS.has(path.basename(stage[k].v))) { k++; continue; }
    break;
  }
  // Drop redirections and their filenames; neither is an argument to the command.
  const argv = stage.slice(k).filter((t) => !t.redir && !t.redirTarget).map((t) => t.v);
  return { argv, assignments };
}

// ---------------------------------------------------------------------------
// git argument parsing. The subcommand is not argv[1] — global options precede it.
// ---------------------------------------------------------------------------

const GIT_GLOBAL_WITH_VALUE = new Set(['-C', '-c', '--exec-path', '--git-dir', '--work-tree', '--namespace', '--config-env']);
function parseGit(argv) {
  if (path.basename(argv[0] || '') !== 'git') return null;
  let i = 1;
  let cDir = null;
  let gitDir = null;
  while (i < argv.length) {
    const a = argv[i];
    if (!a.startsWith('-')) break;
    if (a === '-C') { cDir = argv[i + 1] ?? null; i += 2; continue; }
    if (a.startsWith('--git-dir=')) { gitDir = a.slice(10); i++; continue; }
    if (GIT_GLOBAL_WITH_VALUE.has(a)) { i += 2; continue; }
    i++;
  }
  if (i >= argv.length) return null;
  return { cDir, gitDir, sub: argv[i], rest: argv.slice(i + 1) };
}

// Exact-token flag test. NEVER a prefix test: `--amend` and `--allow-empty` both begin
// with the text of `--all`, and reading either as `--all` would refuse ordinary work.
function hasFlag(rest, long, short) {
  for (const a of rest) {
    if (a === '--') break;
    if (a === long) return true;
    if (short && /^-[A-Za-z]+$/.test(a) && a.slice(1).includes(short)) return true;
  }
  return false;
}

// Positional arguments: everything after `--`, or the non-option words before it.
function positionals(rest) {
  const dd = rest.indexOf('--');
  if (dd !== -1) return rest.slice(dd + 1);
  return rest.filter((a) => !a.startsWith('-'));
}

const BROAD_PATHSPECS = new Set(['.', './', ':/', '*', ':/*', '"."']);
const isBroadPath = (p) => BROAD_PATHSPECS.has(p);

// ---------------------------------------------------------------------------
// Shared-checkout detection — the precision mechanism for rules 1-3.
// A working tree is "shared" when its git COMMON dir has linked worktrees registered.
// A `git init` scratch dir has none. A throwaway clone has none. Only a worktree set
// does, and a worktree set is exactly the situation where a tree-wide op robs somebody.
// ---------------------------------------------------------------------------

const sharedCache = new Map();
function isSharedCheckout(dir) {
  if (sharedCache.has(dir)) return sharedCache.get(dir);
  let verdict = false;
  const forced = (process.env.NEXUS_GUARD_ROOTS || '').split(':').filter(Boolean);
  if (forced.some((root) => dir === root || dir.startsWith(root.endsWith('/') ? root : `${root}/`))) {
    verdict = true;
  } else {
    try {
      const r = spawnSync('git', ['-C', dir, 'rev-parse', '--git-common-dir'], { encoding: 'utf8', timeout: 5000 });
      if (r.status === 0) {
        const common = path.resolve(dir, r.stdout.trim());
        const wt = path.join(common, 'worktrees');
        verdict = fs.existsSync(wt) && fs.readdirSync(wt).length > 0;
      }
    } catch { /* not a repo, or git unavailable: nothing to protect */ }
  }
  sharedCache.set(dir, verdict);
  return verdict;
}

// ---------------------------------------------------------------------------
// Rule 4 vocabulary. A transcribed list, and therefore the part of this file most
// likely to lag reality — see KNOWN GAPS. It fails open, never closed.
// ---------------------------------------------------------------------------

const CARGO_GATES = new Set(['test', 'clippy', 'build', 'check', 'fmt', 'bench', 'nextest', 'miri', 'tarpaulin', 'deny', 'audit']);
const BARE_GATES = new Set(['tsc', 'vitest', 'jest', 'eslint', 'pytest', 'mypy', 'ruff', 'cargo-nextest']);
const SCRIPT_RE = /(test|lint|build|check|typecheck|fmt|format|gate|ci|clippy|audit)/i;
const DISCARDS = new Set(['grep', 'egrep', 'fgrep', 'rg', 'ag', 'tail', 'head', 'less', 'more', 'wc', 'sort', 'uniq', 'awk', 'sed', 'cut', 'jq', 'column', 'tr', 'tac', 'nl']);

function isGateCommand(argv) {
  if (!argv.length) return null;
  const base = path.basename(argv[0]);
  if (base === 'gates' || argv[0].endsWith('scripts/gates')) return 'scripts/gates';
  if (base === 'cargo') {
    const sub = argv.slice(1).find((a) => !a.startsWith('-'));
    return sub && CARGO_GATES.has(sub) ? `cargo ${sub}` : null;
  }
  if (base === 'npm' || base === 'pnpm' || base === 'yarn' || base === 'bun') {
    const words = argv.slice(1).filter((a) => !a.startsWith('-'));
    if (words[0] === 'test' || words[0] === 'ci') return `${base} ${words[0]}`;
    if ((words[0] === 'run' || words[0] === 'run-script') && words[1] && SCRIPT_RE.test(words[1])) return `${base} run ${words[1]}`;
    return null;
  }
  if (base === 'npx') {
    const sub = argv.slice(1).find((a) => !a.startsWith('-'));
    return sub && BARE_GATES.has(path.basename(sub)) ? `npx ${sub}` : null;
  }
  if (base === 'node') return argv.includes('--test') ? 'node --test' : null;
  if (BARE_GATES.has(base)) return base;
  if (/\.test\.(sh|mjs|js)$/.test(argv[0])) return path.basename(argv[0]);
  return null;
}

// ---------------------------------------------------------------------------
// Rule 6 — is a verification RUNNING in this worktree right now?
//
// The honest signal, and the reason this rule is shippable at all: a process's cwd in
// /proc is the kernel's own answer, and the entry DISAPPEARS the instant the process
// exits. So "nothing is running" is a fact, not a stale cache — which is the property the
// rule lives or dies on, because arming when nothing is running is the false positive
// that would get all six switched off.
//
// Verified on this machine: cwd exact, elapsed accurate to ~1s against a known sleep, and
// /proc/<pid> gone within 300ms of the process ending.
//
// WHAT IT MISSES, stated rather than implied:
//   - Linux only. There is no /proc on macOS, so this rule NEVER arms there and the other
//     five are unaffected. It fails open, silently, by design.
//   - A runner that chdir'd away from the worktree after starting, or one started from a
//     parent directory with --manifest-path/--prefix pointing inward. Its cwd is then not
//     inside the tree and it is invisible here. (60 of 205 processes on this box have an
//     unreadable cwd — all other users'; a run the operator started is this user's.)
//   - Only the named verification runners count (the same transcribed list rule 4 uses),
//     so a gate invoked some other way is not seen. Fails open, never closed.
//   - Our own ancestors are excluded: you cannot be interrupted by the process that
//     spawned you, and without this the suite would detect its own `node --test` run.
// ---------------------------------------------------------------------------

// A /proc cmdline is not a typed command line: the kernel records the EXECUTED image, so
// `scripts/gates` (which is `#!/usr/bin/env node`) appears as `node /path/scripts/gates`,
// and vitest as `node .../node_modules/.bin/vitest`. Matching argv[0] alone misses every
// one of them — the first version of this did, and the suite caught it. Strip interpreters
// and leading flags, then ask the ordinary matcher.
const INTERPRETERS = new Set(['node', 'nodejs', 'sh', 'bash', 'dash', 'zsh', 'python', 'python3', 'ruby', 'deno', 'bun', 'env']);
const CLI_ALIASES = new Map([['npm-cli.js', 'npm'], ['npx-cli.js', 'npx'], ['yarn.js', 'yarn'], ['pnpm.cjs', 'pnpm']]);

function isVerificationProcess(argv) {
  const direct = isGateCommand(argv);
  if (direct) return direct;
  let rest = argv;
  for (let hop = 0; hop < 4 && rest.length > 1; hop++) {
    const base = path.basename(rest[0]);
    if (!INTERPRETERS.has(base)) break;
    rest = rest.slice(1);
    while (rest.length && rest[0].startsWith('-')) rest = rest.slice(1); // node --experimental-x
    if (!rest.length) return null;
    const alias = CLI_ALIASES.get(path.basename(rest[0]));
    const probe = alias ? [alias, ...rest.slice(1)] : rest;
    const hit = isGateCommand(probe);
    if (hit) return hit;
  }
  return null;
}

function ancestorPids() {
  const out = new Set();
  let pid = process.pid;
  for (let hop = 0; hop < 32 && pid > 0; hop++) {
    out.add(pid);
    try {
      const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
      // Field 4 is ppid, but field 2 (comm) may contain spaces/parens — split after ')'.
      pid = Number(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
    } catch { break; }
  }
  return out;
}

let procScan = null;
function verificationProcs() {
  if (procScan) return procScan;
  procScan = [];
  let uptime;
  try {
    uptime = parseFloat(fs.readFileSync('/proc/uptime', 'utf8').split(' ')[0]);
  } catch {
    return procScan; // no /proc: this rule stands down entirely
  }
  const mine = ancestorPids();
  let names;
  try { names = fs.readdirSync('/proc'); } catch { return procScan; }
  for (const name of names) {
    if (!/^\d+$/.test(name)) continue;
    const pid = Number(name);
    if (mine.has(pid)) continue;
    let cwd;
    try { cwd = fs.readlinkSync(`/proc/${pid}/cwd`); } catch { continue; }
    let argv;
    try {
      argv = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean);
    } catch { continue; }
    if (!argv.length) continue;
    const gate = isVerificationProcess(argv);
    if (!gate) continue;
    let seconds = 0;
    try {
      const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
      const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
      seconds = Math.max(0, Math.round(uptime - Number(fields[19]) / 100));
    } catch { /* elapsed is a nicety, not the signal */ }
    procScan.push({ pid, cwd, gate, seconds });
  }
  return procScan;
}

const overlaps = (a, b) => a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`);

function runsTouching(dir) {
  // A runner spawns children sharing its cwd (npm -> sh -> vitest). Keep the OLDEST per
  // cwd: that is the top-level run, and reporting three lines for one gate is noise.
  const hits = verificationProcs().filter((p) => overlaps(p.cwd, dir));
  const byCwd = new Map();
  for (const h of hits) {
    const prev = byCwd.get(h.cwd);
    if (!prev || h.seconds > prev.seconds) byCwd.set(h.cwd, h);
  }
  return [...byCwd.values()].sort((a, b) => b.seconds - a.seconds);
}

const humanElapsed = (s) => (s >= 60 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${s}s`);

// Tree-mutating git subcommands. `commit` and `add` are deliberately ABSENT: they change
// the index and refs, not the files a running gate is compiling.
const TREE_MUTATING_GIT = new Set([
  'merge', 'rebase', 'cherry-pick', 'revert', 'pull', 'checkout', 'switch',
  'reset', 'restore', 'apply', 'am', 'stash', 'clean',
]);

function inFlightFinding(dir, what) {
  const runs = runsTouching(dir);
  if (!runs.length) return null;
  const detail = runs.map((r) => `${r.gate} (pid ${r.pid}, running ${humanElapsed(r.seconds)}, cwd ${r.cwd})`).join('; ');
  return {
    rule: 6, level: 'ask', override: 'NEXUS_ALLOW_MUTATE_DURING_RUN',
    what: `${what} while a verification is IN FLIGHT in this worktree — ${detail}`,
    why: 'changing files under a running gate makes it report on a tree that no longer exists, and the reds it invents are indistinguishable from real ones — on 2026-09-15 a merge into a worktree mid-run produced five reds, four of them manufactured, and the result was briefly believed',
    instead: `let it finish and read its result first, or do this in a separate worktree; if you mean to abandon the run, kill ${runs.map((r) => r.pid).join(' ')} first so nobody reads its output as a verdict`,
  };
}

// ---------------------------------------------------------------------------
// Overrides. Read from the command text first (the Bash tool does not persist env
// between calls, so an `export` in an earlier call could never reach this process),
// and from the real environment second.
// ---------------------------------------------------------------------------

function overrideActive(name, rawCommand) {
  const fromEnv = process.env[name];
  if (fromEnv && fromEnv !== '0' && fromEnv !== '') return true;
  return new RegExp(`(^|[\\s;&(])(export\\s+)?${name}=(?!0\\b|["']?\\s)[^\\s;&|]+`).test(rawCommand);
}

// ---------------------------------------------------------------------------
// The checks.
// ---------------------------------------------------------------------------

const MUTATING_GIT = new Set([
  'commit', 'add', 'rm', 'mv', 'push', 'merge', 'rebase', 'cherry-pick', 'revert',
  'reset', 'checkout', 'switch', 'restore', 'tag', 'stash', 'apply', 'am', 'clean',
  'worktree', 'gc', 'filter-branch', 'update-ref', 'notes', 'submodule', 'pull',
]);

function checkDestructive(g, where) {
  const R = (what, why, instead) => ({ what, why, instead });
  if (g.sub === 'reset' && hasFlag(g.rest, '--hard')) {
    return R('git reset --hard',
      'it discards every uncommitted change in this shared checkout, including work belonging to the other agents live in it right now — and uncommitted work leaves no git object, so there is no reflog entry and nothing to recover',
      `git -C ${where} reset --soft HEAD~1  (moves the branch pointer and leaves every working tree alone)`);
  }
  if (g.sub === 'checkout' || g.sub === 'restore') {
    const staged = hasFlag(g.rest, '--staged', 'S');
    const worktree = hasFlag(g.rest, '--worktree', 'W');
    if (g.sub === 'restore' && staged && !worktree) return null; // unstaging destroys nothing on disk
    const paths = positionals(g.rest);
    const broad = paths.some(isBroadPath) || paths.some((p) => path.resolve(where, p) === path.resolve(where));
    const forced = g.sub === 'checkout' && hasFlag(g.rest, '--force', 'f') && !g.rest.includes('--');
    if (broad || forced) {
      return R(`git ${g.sub} ${paths.join(' ') || '--force'}`,
        'it overwrites the whole working tree from HEAD, destroying the uncommitted edits of every agent sharing this checkout, with nothing to recover them from',
        `name the files you actually mean: git -C ${where} ${g.sub} -- <your paths>`);
    }
    return null;
  }
  if (g.sub === 'clean' && hasFlag(g.rest, '--force', 'f') && !hasFlag(g.rest, '--dry-run', 'n')) {
    return R('git clean -f',
      'it deletes untracked files across the tree — which in a shared checkout means another agent\'s new, never-committed source files, unrecoverably',
      `preview first with git -C ${where} clean -nd, or do the cleaning in a throwaway clone`);
  }
  if (g.sub === 'stash') {
    const sub2 = g.rest.find((a) => !a.startsWith('-'));
    if (sub2 === 'pop') {
      return R('git stash pop',
        'the stash stack is shared with every worktree, so stash@{0} is very often somebody else\'s work — and pop removes the entry, so a bad apply cannot be retried',
        'find your own entry by tag (git stash list --format="%H %gs"), then git stash apply <sha>');
    }
    if (sub2 === 'clear') {
      return R('git stash clear',
        'it discards every stash entry in the repository, including entries other agents are relying on, with no way back',
        'drop only your own entry, found by its unique tag');
    }
    if (!sub2 || sub2 === 'push' || sub2 === 'save') {
      if (!hasFlag(g.rest, '--message', 'm')) {
        return R('bare git stash',
          'it sweeps every concurrent agent\'s uncommitted work off the tree and reports success, so the loss is only discovered later by whoever lost it — this is the trap, not the obvious one',
          `set work aside with a WIP commit instead; if you truly need the stack, git -C ${where} stash push -u -m "<unique-tag>" and recover with apply <sha>, never pop`);
      }
    }
  }
  return null;
}

function checkBroadStaging(g, where) {
  if (g.sub === 'add') {
    const all = hasFlag(g.rest, '--all', 'A') || hasFlag(g.rest, '--no-ignore-removal');
    const update = hasFlag(g.rest, '--update', 'u');
    const paths = positionals(g.rest);
    const scoped = paths.length > 0 && !paths.some(isBroadPath);
    if (scoped) return null;
    if (all || update || paths.some(isBroadPath)) {
      const shown = all ? 'git add -A' : update ? 'git add -u' : 'git add .';
      return {
        what: shown,
        why: 'the staging area is shared state — between your add and your commit, a concurrent agent\'s commit sweeps up everything you just staged, into a message describing none of it; both sets of gates pass and nobody notices until someone reads the log',
        instead: `git -C ${where} add <your paths>  then  git -C ${where} commit -- <those paths>  (confirm with git diff --cached --name-only first)`,
      };
    }
    return null;
  }
  if (g.sub === 'commit' && hasFlag(g.rest, '--all', 'a')) {
    return {
      what: 'git commit -a',
      why: 'it stages every modified tracked file in the checkout, so a sibling agent\'s in-flight edits ride along inside your commit — note that path staging alone would not have saved you either, since a co-writer\'s hunk in a file you also touched still rides along',
      instead: `git -C ${where} commit -- <the paths you changed>  (and verify the COMMITTED tree, not the working tree)`,
    };
  }
  return null;
}

function checkGitCwd(g, cdSeen) {
  if (g.cDir || g.gitDir || cdSeen) return null;
  if (!MUTATING_GIT.has(g.sub)) return null;
  return {
    what: `git ${g.sub}  (no -C <absolute path>)`,
    why: 'a shell cwd persists between calls, so the edit lands in the right tree while the write runs in the wrong one — and a branch name resolves from any worktree, so a push can "succeed" carrying a tip that does not contain your fix',
    instead: `git -C <absolute path to your worktree> ${g.sub} ...`,
  };
}

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

function decide(command, cwd) {
  const findings = [];
  const tokens = tokenize(command);

  // Rule 4/5 look at pipelines; rules 1-3 at individual stages, with cwd tracked
  // across `cd` so a scratch directory is judged as a scratch directory.
  let runningCwd = cwd;
  let cdSeenThisLine = false;

  for (const pipe of pipelines(tokens)) {
    const stageInfo = pipe.stages.map(argvOf);

    // Rule 4: a gate on the left of a pipe, a status-discarding filter on the right.
    if (stageInfo.length > 1 && !/\bset\s+-[A-Za-z]*o?\s*pipefail|\bset\s+-o\s+pipefail/.test(command)) {
      const gate = isGateCommand(stageInfo[0].argv);
      if (gate) {
        const sink = stageInfo.slice(1).map((s) => path.basename(s.argv[0] || '')).find((b) => DISCARDS.has(b));
        if (sink) {
          findings.push({
            rule: 4, level: 'block', override: 'NEXUS_ALLOW_PIPED_GATE',
            what: `${gate} piped into ${sink}`,
            why: 'without `set -o pipefail` a pipeline reports the RIGHT-hand command\'s status, so the gate\'s own exit code is thrown away and a red run reads green — this exact shape shipped a faked-green gate in release 1.10.3',
            instead: `${gate} > <unique-name>.log 2>&1 ; echo "exit=$?"   then read the file (or prefix the pipeline with set -o pipefail)`,
          });
        }
      }
    }

    for (const { argv, assignments } of stageInfo) {
      if (!argv.length) continue;
      const base = path.basename(argv[0]);

      // Track `cd` so a scratch dir is judged as a scratch dir, not as this worktree.
      if (base === 'cd' && argv[1] && !argv[1].startsWith('-')) {
        runningCwd = path.resolve(runningCwd, argv[1]);
        if (path.isAbsolute(argv[1])) cdSeenThisLine = true;
        continue;
      }

      // Rule 5: --allow-partial anywhere it can be typed.
      if ((base === 'gates' || argv[0].endsWith('scripts/gates')) && argv.includes('--allow-partial')) {
        findings.push({
          rule: 5, level: 'block', override: 'NEXUS_ALLOW_PARTIAL_GATES',
          what: 'scripts/gates --allow-partial',
          why: 'it turns exit 3 ("everything that ran passed, but coverage was INCOMPLETE") into exit 0 — a control run had a missing binary exit 0 with the deploy proceeding; its one sanctioned site is remote-staging.yml, not a command line',
          instead: 'run `scripts/gates` and read the PARTIAL list it prints; fix or explicitly scope with --job <names> so the incomplete part is named, not swallowed',
        });
        continue;
      }

      const g = parseGit(argv);
      if (!g) continue;
      const where = path.resolve(runningCwd, g.cDir || '.');

      // Rule 6 is NOT gated on a shared checkout: a run in flight can be clobbered in a
      // solo tree just as easily. Its precondition is that something is actually running.
      if (TREE_MUTATING_GIT.has(g.sub)) {
        const f = inFlightFinding(where, `git ${g.sub}`);
        if (f) findings.push(f);
      }

      if (!isSharedCheckout(where)) continue; // scratch repo / throwaway clone: nobody to rob

      const d = checkDestructive(g, where);
      if (d) findings.push({ rule: 1, level: 'block', override: 'NEXUS_ALLOW_DESTRUCTIVE', ...d });

      const b = checkBroadStaging(g, where);
      if (b) findings.push({ rule: 2, level: 'block', override: 'NEXUS_ALLOW_BROAD_ADD', ...b });

      const c = checkGitCwd(g, cdSeenThisLine);
      if (c) findings.push({ rule: 3, level: 'ask', override: 'NEXUS_ALLOW_GIT_CWD', ...c });

      void assignments;
    }
  }

  const live = findings.filter((f) => !overrideActive(f.override, command));
  if (!live.length) return null;
  const blocking = live.filter((f) => f.level === 'block');
  return { level: blocking.length ? 'block' : 'ask', findings: blocking.length ? blocking : live };
}

function render(verdict) {
  const verb = verdict.level === 'block' ? 'REFUSED' : 'CONFIRM';
  return verdict.findings
    .map((f) => [
      `nexus-guard: ${verb} — ${f.what}`,
      `nexus-guard: why: ${f.why}.`,
      `nexus-guard: instead: ${f.instead}`,
      `nexus-guard: deliberate exception: prefix the command with ${f.override}=1`,
    ].join('\n'))
    .join('\n\n');
}

// The file-editing tools. Rule 6 reaches them because an Edit to a tracked file under a
// running gate corrupts that run exactly as a merge does — it is the same act with a
// different verb. Only rule 6 applies here; the other five are about shell commands.
const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);

function decideEdit(payload) {
  const target = payload?.tool_input?.file_path || payload?.tool_input?.notebook_path;
  if (typeof target !== 'string' || !target) return null;
  const abs = path.resolve(payload.cwd || process.cwd(), target);
  const f = inFlightFinding(path.dirname(abs), `${payload.tool_name} ${path.basename(abs)}`);
  if (!f) return null;
  // No command text to carry an override here, so this one is env-only. Stated in the
  // header as a real limitation rather than left for someone to discover.
  if (overrideActive(f.override, '')) return null;
  return { level: 'ask', findings: [f] };
}

function main(payload) {
  if (EDIT_TOOLS.has(payload.tool_name)) {
    const v = decideEdit(payload);
    if (!v) return 0;
    process.stdout.write(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'ask',
        permissionDecisionReason: render(v),
      },
    }));
    return 0;
  }
  if (payload.tool_name !== 'Bash') return 0;
  const command = payload?.tool_input?.command;
  if (typeof command !== 'string' || !command.trim()) return 0;
  const cwd = payload.cwd || process.cwd();

  const verdict = decide(command, cwd);
  if (!verdict) return 0;

  const message = render(verdict);
  if (verdict.level === 'block') {
    process.stderr.write(`${message}\n`);
    return 2;
  }
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: 'ask',
      permissionDecisionReason: message,
    },
  }));
  return 0;
}

let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (d) => { raw += d; });
process.stdin.on('end', () => {
  let code = 0;
  try {
    code = main(JSON.parse(raw || '{}'));
  } catch (err) {
    // Never block on our own bug, never stay silent about it either.
    process.stdout.write(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'ask',
        permissionDecisionReason: `nexus-guard: the guard itself errored (${err.message}) and could not judge this command. It is NOT vouching for it.`,
      },
    }));
    code = 0;
  }
  process.exitCode = code;
});
