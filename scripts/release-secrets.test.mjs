// Tests for .github/workflows/release.yml: no step that builds or runs third-party code can
// see a secret.
//
// WHY. The release compiles and runs code nobody here wrote: FFTW and Hamlib from their
// release tarballs, every crates.io build script and proc macro, npm's install scripts, the
// AppImage tooling. Until this file existed it did so in the same steps that held the updater
// signing key, the Apple notarization key and the ClubLog key, with the Apple signing identity
// already unlocked in a keychain, so a tampered tarball or dependency could read a key straight
// out of its own environment. The operator's ruling (2026-09-29): third-party builds run before
// the key is loaded, and the key only touches the finished installer. This file holds that down.
//
// WHAT IT CHECKS, all of it read out of the workflow at run time:
//   1. No secret sits in a workflow- or job-level `env:`. Those reach every step of the job,
//      the actions and the builds included.
//   2. SEEN below is the complete list of which step sees which secret's value, and no step
//      that builds or runs third-party code sees one, except CLUBLOG_API_KEY in the steps that
//      compile it into the app (see COMPILED_IN).
//   3. No third-party step runs while the Apple signing identity is in a keychain: it is
//      imported after every build and deleted once the bundle is signed.
//   4. A step that holds a secret does not hand it on through $GITHUB_ENV or $GITHUB_OUTPUT.
//   5. No JOB that builds or runs third-party code holds a signing secret, whichever of its steps
//      holds it: the updater key signs the Windows and Linux installers in a job of its own, on a
//      runner that builds nothing. macOS is the one exception (see SIGNS_IN_BUNDLER).
//   6. Every artifact a job downloads is uploaded by a job it needs, so splitting a job cannot
//      leave a later step reading a file nothing hands it.
//
// WHAT IT CANNOT CHECK. Step scoping keeps a key out of a build's environment. It is not a
// boundary against code that sets out to find the key: a hosted runner is handed the secrets
// its job references when the job starts, and every step on it has passwordless sudo, which is
// how the tj-actions/changed-files compromise (March 2025) read secrets out of the runner's
// memory. Only a separate job, on a separate machine, is that boundary. Nor can a test over the
// YAML see inside the scripts a step runs: a step counts as third-party by the commands it
// names, listed in THIRD_PARTY.
//
// Every "no violation" here is paired with a planted violation that MUST be reported, so a
// reader that stopped seeing steps cannot pass by seeing nothing.
//
// Run: node --test scripts/release-secrets.test.mjs
// RELEASE_WORKFLOW=<path> reads a different copy of the workflow.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WORKFLOW =
  process.env.RELEASE_WORKFLOW || path.join(ROOT, '.github', 'workflows', 'release.yml');
const TEXT = fs.readFileSync(WORKFLOW, 'utf8');

// Which step sees which secret's VALUE: every one of them, so this table is the map. A presence
// test (`secrets.X != ''`, which the Verify steps use to decide whether a signature is required)
// hands a step one bit rather than the key, and is not listed.
const DEEPCW = 'Stage the DeepCW model (not committed — AGPL-3.0 (c) e04)';
const IMPORT = 'Import the signing certificate into an ephemeral keychain';
const BUNDLE = 'Bundle, sign and notarize the DMG';
const SIGN_STEP = 'Sign the NSIS installer and the AppImage';
const SIGN = [
  ['sign-updaters', SIGN_STEP],
  ['macos', BUNDLE],
];
const SEEN = {
  TAURI_SIGNING_PRIVATE_KEY: SIGN,
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: SIGN,
  APPLE_CERTIFICATE: [['macos', IMPORT]],
  APPLE_CERTIFICATE_PASSWORD: [['macos', IMPORT]],
  APPLE_API_KEY_ID: [['macos', BUNDLE]],
  APPLE_API_ISSUER_ID: [['macos', BUNDLE]],
  APPLE_API_KEY_CONTENT: [['macos', BUNDLE]],
  // Another step that compiles the key in (the Linux build step and the Pi container build, #388)
  // gets its row here in the change that hands it the key; COMPILED_IN lets that step be third-party.
  CLUBLOG_API_KEY: [
    ['windows', 'Cross-build the NSIS installer'],
    ['macos', 'Compile the app'],
  ],
  // `${{ github.token }}`: read-only in the build jobs, contents: write in the two publish jobs.
  GITHUB_TOKEN: [
    ['ci-evidence', 'Require a completed, successful ci.yml run on this exact commit'],
    ['linux-x86', DEEPCW],
    ['windows', DEEPCW],
    ['macos', DEEPCW],
    ['pi', DEEPCW],
    ['publish', 'Create the GitHub Release'],
    ['publish', 'Hand off to the downstream publish (SourceForge + site)'],
    ['publish-pi', 'Upload to the release + extend SHA256SUMS'],
  ],
};

// The one exception to "a third-party step sees no secret". CLUBLOG_API_KEY is baked into the
// binary by option_env! (src-tauri/src/lib.rs, crates/propagation/src/live/dxped.rs), so the
// cargo run that compiles Nexus has to see it, and that same run compiles every crates.io build
// script and proc macro. The key can be read out of any shipped binary anyway, so a build that
// reads it learns nothing a download does not. No signing secret gets this exception. Another
// key compiled in by option_env! (NEXUS_RB_CLIENT_KEY in repeaterbook.rs is one; no release job
// sets it today) joins this set in the change that wires it, not before.
const COMPILED_IN = new Set(['CLUBLOG_API_KEY']);

// Rule 5's exception. Apple's codesign and notarization run inside tauri's bundler, which makes
// and signs the macOS updater tarball in the same run, so the macOS job cannot hand its keys to a
// job of its own. It keeps them at step scope instead: every compile first, the certificate
// imported after the last build, the keychain deleted once the bundle is signed (rule 3).
const SIGNS_IN_BUNDLER = new Set(['macos']);

// Every secret but these is signing-grade for rule 5: the compiled-in keys, and the job token
// (read-only in the build jobs).
const signingGrade = (secret) => !COMPILED_IN.has(secret) && secret !== 'GITHUB_TOKEN';

// GitHub's own checkout and artifact actions. They are the platform the runner already is, so they
// do not make a JOB third-party for rule 5; a step running one is still never handed a secret.
const PLATFORM = /^actions\/(?:checkout|download-artifact|upload-artifact)@/;

// What makes a step third-party: code nobody here wrote runs in it. Matched against the step's
// commands with its shell comment lines removed. A `uses:` step always counts, because an
// action's code lives in another repository. `cargo tauri bundle` and `cargo tauri signer` are
// deliberately absent: they are the two tauri-cli commands a keyed step runs, and neither
// compiles anything (tauri-cli 2.11.5, src/bundle.rs and src/signer/sign.rs).
// A script counts where it is RUN (at a command position), not where an echo names it.
const RUNS = String.raw`(?:^|[;&|(]|\s)(?:bash\s+|sh\s+)?(?:\.\/)?`;
const THIRD_PARTY = [
  [new RegExp(`${RUNS}scripts/build-(?:linux|windows-cross)\\.sh\\b`, 'm'), 'a platform build script (native libraries, npm, crates.io, the bundler)'],
  [new RegExp(`${RUNS}scripts/fetch-hamlib-unix\\.sh\\b`, 'm'), 'the Hamlib build (configure and make from its release tarball)'],
  [/\bcargo\s+(?:build|check|clippy|install|run|rustc|test)\b/, 'cargo compiling crates.io code'],
  [/\bcargo\s+tauri\s+(?:build|dev)\b/, '`cargo tauri build` (every crate, and the UI build)'],
  [/\bnpm\s+(?:--prefix\s+\S+\s+)?(?:ci|exec|i|install|run|test)\b|\bnpx\s/, 'npm'],
  [/\bdocker\s+(?:buildx\s+)?build\b/, 'a container build'],
  [/\bbrew\s+install\b/, 'Homebrew'],
  [/\bapt(?:-get)?\s+install\b/, 'apt (package scripts run as root)'],
  [/\bpip3?\s+install\b|\bpython3?\s+-m\s+pip\s+install\b/, 'pip'],
  [/(?:^|[\s;&|(])\.\/configure\b|(?:^|[\s;&|(])make(?:\s|$)/m, 'configure and make'],
  [/--appimage-extract\b/, 'the AppImage runtime'],
  [/\brigctld"?\s+--version\b/, 'rigctld (built from the Hamlib tarball)'],
  [/\bjava\s+-jar\b/, 'a downloaded jar'],
  [/\bcurl\b[^\n]*\|\s*(?:ba)?sh\b/, 'a downloaded install script'],
];

function thirdParty(step) {
  if (step.uses) return `the action ${step.uses}`;
  const code = (step.run ?? '')
    .split('\n')
    .filter((l) => !/^\s*#/.test(l))
    .join('\n');
  for (const [re, what] of THIRD_PARTY) if (re.test(code)) return what;
  return null;
}

// A reader for the YAML this repo's workflows are written in: block mappings, block sequences,
// `|` and `>` block scalars, one-line plain or quoted values. Anything else is refused by line
// number rather than guessed at (tabs, anchors and aliases, a flow collection or a quoted value
// that runs onto the next line), because a line it misplaced is a secret it could file under the
// wrong step. Every line it reads becomes one entry: a path (`jobs.windows.steps.3.env.X`), the
// value, and the line number.
function readWorkflow(text, file = path.basename(WORKFLOW)) {
  const lines = text.split('\n');
  const entries = [];
  const items = []; // { job, index, line } for each step item, to check attribution
  const stack = []; // { indent, label, items }
  const refuse = (n, why) => {
    throw new Error(`${file}:${n + 1}: ${why} — this reader does not model it\n  ${lines[n]}`);
  };
  const scalar = (n, value) => {
    if (/^[&*]/.test(value)) refuse(n, 'a YAML anchor or alias');
    if (/^[[{]/.test(value) && !/[\]}]\s*(?:#.*)?$/.test(value)) refuse(n, 'a flow collection that continues');
    if (/^"/.test(value) && !/^"(?:[^"\\]|\\.)*"\s*(?:#.*)?$/.test(value)) refuse(n, 'a quoted value that continues');
    if (/^'/.test(value) && !/^'(?:[^']|'')*'\s*(?:#.*)?$/.test(value)) refuse(n, 'a quoted value that continues');
    return value;
  };
  for (let n = 0; n < lines.length; n++) {
    const raw = lines[n];
    if (/^\s*$/.test(raw)) continue;
    if (/^ *\t/.test(raw)) refuse(n, 'a tab in the indentation');
    if (/^\s*#/.test(raw)) {
      entries.push({ path: ['#'], value: raw, line: n + 1 });
      continue;
    }
    let col = raw.match(/^ */)[0].length;
    let body = raw.slice(col);
    const dash = body.match(/^-( +)/);
    if (dash) {
      while (stack.length && stack.at(-1).indent >= col) stack.pop();
      const parent = stack.at(-1);
      if (!parent) refuse(n, 'a sequence item outside any mapping');
      const index = parent.items++;
      stack.push({ indent: col, label: String(index), items: 0 });
      if (stack.length === 4 && stack[0].label === 'jobs' && stack[2].label === 'steps') {
        items.push({ job: stack[1].label, index, line: n + 1 });
      }
      col += 1 + dash[1].length;
      body = body.slice(1 + dash[1].length);
      if (!/^[A-Za-z0-9_.-]+:(?: |$)/.test(body)) {
        entries.push({ path: stack.map((f) => f.label), value: scalar(n, body), line: n + 1 });
        continue;
      }
    } else if (body === '-') {
      refuse(n, 'a bare sequence dash');
    }
    const kv = body.match(/^([A-Za-z0-9_.-]+):(?: +(.*))?$/);
    if (!kv) refuse(n, 'a line that is not `key:`, `key: value` or `- item`');
    if (kv[1] === '<<') refuse(n, 'a YAML merge key');
    while (stack.length && stack.at(-1).indent >= col) stack.pop();
    stack.push({ indent: col, label: kv[1], items: 0 });
    const pathHere = stack.map((f) => f.label);
    const value = (kv[2] ?? '').trimEnd();
    if (/^[|>][-+]?[0-9]?\s*(?:#.*)?$/.test(value)) {
      const block = [];
      while (n + 1 < lines.length) {
        const next = lines[n + 1];
        if (!/^\s*$/.test(next) && next.match(/^ */)[0].length <= col) break;
        block.push(next);
        n++;
      }
      entries.push({ path: pathHere, value: block.join('\n'), line: n + 1 - block.length });
    } else if (value !== '' && !/^#/.test(value)) {
      entries.push({ path: pathHere, value: scalar(n, value), line: n + 1 });
    }
  }
  return model(entries, items);
}

// Every `${{ … }}` that names a secret or the job token, classified. A plain `secrets.NAME` (or
// `github.token`) hands the value over; `secrets.NAME != ''` hands over one bit. Anything else
// that mentions them (`toJSON(secrets)`, a format() of one, `secrets[...]`) is not something this
// test can reason about and is reported as such.
const EXPR = /\$\{\{([\s\S]*?)\}\}/g;
const MENTIONS = /\bsecrets\b|\bgithub\.token\b/;
function refsIn(value) {
  const out = [];
  for (const [, expr] of value.matchAll(EXPR)) {
    if (!MENTIONS.test(expr)) continue;
    let m;
    if ((m = expr.match(/^\s*secrets\.([A-Za-z_][A-Za-z0-9_]*)\s*$/))) out.push({ secret: m[1], kind: 'value', expr });
    else if (/^\s*github\.token\s*$/.test(expr)) out.push({ secret: 'GITHUB_TOKEN', kind: 'value', expr });
    else if ((m = expr.match(/^\s*secrets\.([A-Za-z_][A-Za-z0-9_]*)\s*[!=]=\s*''\s*$/))) out.push({ secret: m[1], kind: 'presence', expr });
    else out.push({ secret: null, kind: 'unclassified', expr });
  }
  return out;
}

function model(entries, items) {
  const jobs = new Map();
  const job = (id) => {
    if (!jobs.has(id)) jobs.set(id, { id, steps: [], refs: [], needs: [] });
    return jobs.get(id);
  };
  const workflowRefs = [];
  const refs = [];
  for (const e of entries) {
    const [a, b, c, d, f, g] = e.path;
    if (a === 'jobs' && b !== undefined) {
      const j = job(b);
      // `needs: x`, `needs: [x, y]`, or a block list of them.
      if (c === 'needs') j.needs = [...(j.needs ?? []), ...e.value.replace(/^\[|\]$/g, '').split(',').map((x) => x.trim()).filter(Boolean)];
      if (c === 'steps' && d !== undefined) {
        const s = (j.steps[Number(d)] ??= { index: Number(d), env: new Map(), with: new Map(), entries: [] });
        s.entries.push(e);
        if (f === 'env' && g !== undefined) s.env.set(g, e.value);
        else if (f === 'with' && g !== undefined) s.with.set(g, e.value);
        else if (['name', 'uses', 'run'].includes(f) && g === undefined) s[f] = e.value;
      }
    }
    for (const r of refsIn(e.value)) {
      const ref = { ...r, path: e.path, line: e.line };
      refs.push(ref);
      if (a === '#') ref.scope = 'comment';
      else if (a === 'env' && c === undefined) ref.scope = 'workflow';
      else if (a === 'jobs' && c === 'env' && d !== undefined && f === undefined) ref.scope = 'job';
      else if (a === 'jobs' && c === 'steps' && ['env', 'with', 'run'].includes(f)) ref.scope = 'step';
      else ref.scope = 'elsewhere';
      if (ref.scope === 'workflow') workflowRefs.push(ref);
      if (ref.scope === 'job') job(b).refs.push(ref);
      if (ref.scope === 'step') job(b).steps[Number(d)].refs = [...(job(b).steps[Number(d)].refs ?? []), ref];
    }
  }
  for (const j of jobs.values()) {
    for (const s of j.steps) {
      if (!s) continue;
      s.label = s.name ?? s.uses ?? `step ${s.index}`;
      // What the step can read: the workflow's env, its job's env, and its own env/with/run.
      s.sees = new Set(
        [...workflowRefs, ...j.refs, ...(s.refs ?? [])]
          .filter((r) => r.kind === 'value')
          .map((r) => r.secret),
      );
    }
  }
  return { jobs, refs, entries, items };
}

function violations(wf) {
  const v = [];
  const step = (jobId, name) => wf.jobs.get(jobId)?.steps.find((s) => s?.name === name);
  for (const r of wf.refs) {
    const at = `line ${r.line} (${r.path.join('.')})`;
    if (r.scope === 'comment') continue;
    if (r.kind === 'unclassified') {
      v.push(`${at}: \`\${{${r.expr}}}\` uses a secret in a way this test cannot classify; pass it as a plain \${{ secrets.NAME }} in the env of the step that needs it`);
    } else if (r.kind === 'value' && r.scope === 'workflow') {
      v.push(`${at}: ${r.secret} in the workflow-level env reaches every step of every job; scope it to the step that uses it`);
    } else if (r.kind === 'value' && r.scope === 'job') {
      v.push(`${at}: ${r.secret} in job ${r.path[1]}'s env reaches every step of it, the third-party builds included; scope it to the step that uses it`);
    } else if (r.scope === 'elsewhere') {
      v.push(`${at}: a secret outside any env, with or run, where this test cannot say who reads it`);
    }
  }
  for (const j of wf.jobs.values()) {
    for (const s of j.steps) {
      if (!s) continue;
      const tp = thirdParty(s);
      for (const secret of s.sees) {
        const listed = (SEEN[secret] ?? []).some(([jid, name]) => jid === j.id && name === s.name);
        if (tp && !(COMPILED_IN.has(secret) && listed)) {
          v.push(`job ${j.id}, step "${s.label}": ${tp} runs in a step that is handed ${secret}`);
        } else if (!listed) {
          v.push(`job ${j.id}, step "${s.label}": sees ${secret}, which SEEN does not list for it`);
        }
      }
      // A key re-exported to $GITHUB_ENV or $GITHUB_OUTPUT reaches every later step.
      const held = [...s.env].filter(([, value]) => refsIn(value).some((r) => r.kind === 'value')).map(([k]) => k);
      for (const line of (s.run ?? '').split('\n')) {
        if (!/GITHUB_(?:ENV|OUTPUT|PATH|STEP_SUMMARY)/.test(line)) continue;
        const named = held.find((k) => new RegExp(`\\$\\{?${k}\\b`).test(line));
        if (named || refsIn(line).some((r) => r.kind === 'value')) {
          v.push(`job ${j.id}, step "${s.label}": writes ${named ?? 'a secret'} where the steps after it can read it`);
        }
      }
    }
    // Persistent signing state: an identity imported into a keychain stays usable by every later
    // step until the keychain is deleted, whatever each step's env says.
    let open = null;
    for (const s of j.steps) {
      if (!s) continue;
      const run = s.run ?? '';
      if (/\bsecurity\s+(?:create-keychain|import)\b/.test(run)) {
        open = s;
      } else if (open && thirdParty(s)) {
        v.push(`job ${j.id}, step "${s.label}": ${thirdParty(s)} runs while the signing identity imported by "${open.label}" is still in a keychain`);
      }
      if (/\bsecurity\s+delete-keychain\b/.test(run)) open = null;
    }
    // Rule 5. Step scoping keeps a key out of a build's environment, but code in any step of a job
    // can reach every secret the job references (see WHAT IT CANNOT CHECK), so a job that runs
    // third-party code must not reference a signing secret at all.
    if (!SIGNS_IN_BUNDLER.has(j.id)) {
      const foreign = j.steps.find((s) => s && !PLATFORM.test(s.uses ?? '') && thirdParty(s));
      for (const s of foreign ? j.steps : []) {
        for (const secret of s ? [...s.sees].filter(signingGrade) : []) {
          v.push(`job ${j.id}: step "${s.label}" holds ${secret} on a runner that also runs third-party code (step "${foreign.label}": ${thirdParty(foreign)}); sign in a job that builds nothing`);
        }
      }
    }
  }
  // Rule 6. Every artifact a job downloads comes from a job it needs, directly or through another.
  // A named download of an artifact nothing upstream uploads fails the run; a PATTERN that matches
  // nothing succeeds and leaves the next step without its files, which is the quiet one.
  const uploads = [];
  for (const j of wf.jobs.values()) {
    for (const s of j.steps) {
      if (s && /^actions\/upload-artifact@/.test(s.uses ?? '') && s.with.has('name')) uploads.push({ job: j.id, name: s.with.get('name') });
    }
  }
  const upstream = (id, seen = new Set()) => {
    for (const n of wf.jobs.get(id)?.needs ?? []) if (!seen.has(n)) upstream(n, seen.add(n));
    return seen;
  };
  const glob = (p) => new RegExp(`^${p.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*')}$`);
  for (const j of wf.jobs.values()) {
    const needed = upstream(j.id);
    for (const s of j.steps) {
      if (!s || !/^actions\/download-artifact@/.test(s.uses ?? '')) continue;
      const want = s.with.get('name') ?? s.with.get('pattern');
      if (!want) {
        v.push(`job ${j.id}, step "${s.label}": downloads every artifact of the run; name the ones it reads`);
        continue;
      }
      // An upload named with an expression (`nexus-pi-${{ matrix.base }}`) is one artifact per value.
      const from = uploads.filter((u) => glob(want).test(u.name.replace(/\$\{\{[^}]*\}\}/g, 'X')));
      if (!from.length) v.push(`job ${j.id}, step "${s.label}": downloads "${want}", which no job uploads`);
      else if (!from.some((u) => needed.has(u.job))) {
        v.push(`job ${j.id}, step "${s.label}": downloads "${want}" from job ${[...new Set(from.map((u) => u.job))].join(', ')}, which job ${j.id} does not need`);
      }
    }
  }
  // The table itself: a stale row is a map that lies.
  for (const [secret, rows] of Object.entries(SEEN)) {
    for (const [jid, name] of rows) {
      const s = step(jid, name);
      if (!s) v.push(`SEEN lists ${secret} for job ${jid}, step "${name}", which does not exist`);
      else if (!s.sees.has(secret)) v.push(`SEEN lists ${secret} for job ${jid}, step "${name}", which does not use it`);
    }
  }
  return v;
}

const WF = readWorkflow(TEXT);
const findStep = (wf, jobId, name) => {
  const s = wf.jobs.get(jobId)?.steps.find((x) => x?.name === name);
  assert.ok(s, `no step "${name}" in job ${jobId}`);
  return s;
};
// Replace text that must occur exactly once: a planted violation that silently matched nothing,
// or matched a second copy of the same block in another job, would test nothing.
function plant(text, from, to) {
  const n = text.split(from).length - 1;
  assert.equal(n, 1, `the anchor for this plant occurs ${n} times, not once:\n${from}`);
  return text.replace(from, to);
}
const reported = (text, needle) => {
  const v = violations(readWorkflow(text));
  assert.ok(
    v.some((x) => x.includes(needle)),
    `a planted violation was not reported. Expected a line containing:\n  ${needle}\ngot:\n  ${v.join('\n  ') || '(nothing)'}`,
  );
};

test('the reader accounts for every step and every secret reference in the workflow', () => {
  // Every secret expression in the raw text, found by a regex that knows nothing of YAML, has to
  // be one the reader filed somewhere. A line the reader skipped would make these differ.
  const raw = [...TEXT.matchAll(EXPR)].filter(([, e]) => MENTIONS.test(e)).length;
  assert.ok(raw > 0, 'the workflow names no secret at all; the test is reading the wrong file');
  assert.equal(WF.refs.length, raw, 'the reader filed a different number of secret expressions than the file holds');
  // Every step item, counted the same way (a step is a mapping, so its item opens with a key; a
  // block-style `needs:` list would not), and every entry of a step inside that step's lines.
  const itemLines = TEXT.match(/^ {6}- [A-Za-z0-9_-]+:/gm)?.length ?? 0;
  assert.equal(WF.items.length, itemLines, 'the reader found a different number of steps than the file has');
  for (const j of WF.jobs.values()) {
    const mine = WF.items.filter((i) => i.job === j.id);
    for (const [k, s] of j.steps.entries()) {
      assert.ok(s, `job ${j.id} has a hole at step ${k}`);
      const from = mine[k].line;
      const to = mine[k + 1]?.line ?? Infinity;
      for (const e of s.entries) {
        assert.ok(e.line >= from && e.line < to, `job ${j.id}: line ${e.line} was filed under step ${k}, which spans ${from}..${to}`);
      }
    }
  }
});

test('the build steps are recognised as third-party, and the signing steps are not', () => {
  // The positive control for THIRD_PARTY: a classifier that recognised nothing would find no
  // third-party step holding a secret, and the test below would pass on any workflow.
  const builds = [
    ['linux-x86', 'Build .deb + AppImage'],
    ['linux-x86', 'Verify artifacts'],
    ['windows', 'Cross-build FFTW + libtempo modem exes'],
    ['windows', 'Cross-build the NSIS installer'],
    ['windows', 'Verify installer'],
    ['macos', 'Install build dependencies'],
    ['macos', 'Build the web UI'],
    ['macos', 'Install tauri-cli'],
    ['macos', 'Stage Hamlib (bundled rigctld — no brew install for the user)'],
    ['macos', 'Compile the app'],
    ['macos', 'Verify the artifacts'],
    ['pi', 'Build .deb in a debian:${{ matrix.base }} container'],
    ['updater-signer', 'Install tauri-cli'],
  ];
  for (const [j, name] of builds) {
    assert.ok(thirdParty(findStep(WF, j, name)), `job ${j}, step "${name}" is not recognised as third-party`);
  }
  const keyed = [...SIGN, ['macos', IMPORT], ['macos', 'Sign the bundled Hamlib binaries']];
  for (const [j, name] of keyed) {
    const why = thirdParty(findStep(WF, j, name));
    assert.equal(why, null, `job ${j}, step "${name}" holds a signing secret and runs ${why}`);
  }
});

test('release.yml: no step or job that builds or runs third-party code can see a secret', () => {
  const v = violations(WF);
  assert.deepEqual(v, [], `\n${v.join('\n')}\n`);
});

test('a signing key planted in a build step is reported', () => {
  const planted = plant(
    TEXT,
    '          CLUBLOG_API_KEY: ${{ secrets.CLUBLOG_API_KEY }}\n        run: bash scripts/build-windows-cross.sh\n',
    '          CLUBLOG_API_KEY: ${{ secrets.CLUBLOG_API_KEY }}\n' +
      '          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n' +
      '        run: bash scripts/build-windows-cross.sh\n',
  );
  reported(planted, 'job windows, step "Cross-build the NSIS installer": a platform build script');
  reported(planted, 'runs in a step that is handed TAURI_SIGNING_PRIVATE_KEY');
});

test('a secret moved back to job scope is reported', () => {
  const planted = plant(
    TEXT,
    '    name: macOS (Apple Silicon .dmg)\n    runs-on: macos-14\n',
    '    name: macOS (Apple Silicon .dmg)\n    runs-on: macos-14\n' +
      '    env:\n      CLUBLOG_API_KEY: ${{ secrets.CLUBLOG_API_KEY }}\n',
  );
  reported(planted, "CLUBLOG_API_KEY in job macos's env reaches every step");
  reported(planted, 'job macos, step "Build the web UI": npm runs in a step that is handed CLUBLOG_API_KEY');
});

test('a secret written straight into a build command is reported', () => {
  const planted = plant(
    TEXT,
    '        run: bash scripts/build-linux.sh\n',
    '        run: bash scripts/build-linux.sh "${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}"\n',
  );
  reported(planted, 'job linux-x86, step "Build .deb + AppImage": a platform build script (native libraries, npm, crates.io, the bundler) runs in a step that is handed TAURI_SIGNING_PRIVATE_KEY_PASSWORD');
});

test('the key handed to a Verify step, instead of whether it exists, is reported', () => {
  const planted = plant(
    TEXT,
    "          SIGNING_KEY_PRESENT: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY != '' }}\n        run: |\n          set -euo pipefail\n          bundle=",
    '          SIGNING_KEY_PRESENT: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n        run: |\n          set -euo pipefail\n          bundle=',
  );
  reported(planted, 'job macos, step "Verify the artifacts": rigctld (built from the Hamlib tarball) runs in a step that is handed TAURI_SIGNING_PRIVATE_KEY');
});

test('a secret used in an expression the test cannot classify is reported', () => {
  const planted = plant(
    TEXT,
    '        run: bash scripts/build-linux.sh\n',
    '          EVERYTHING: ${{ toJSON(secrets) }}\n        run: bash scripts/build-linux.sh\n',
  );
  reported(planted, 'uses a secret in a way this test cannot classify');
});

test('a key re-exported to $GITHUB_ENV is reported', () => {
  const planted = plant(
    TEXT,
    '          signer/cargo-tauri signer sign --app-version "$ver" "$exe" >/dev/null\n',
    '          signer/cargo-tauri signer sign --app-version "$ver" "$exe" >/dev/null\n' +
      '          echo "K=$TAURI_SIGNING_PRIVATE_KEY" >> "$GITHUB_ENV"\n',
  );
  reported(planted, `job sign-updaters, step "${SIGN_STEP}": writes TAURI_SIGNING_PRIVATE_KEY where the steps after it can read it`);
});

test('the updater key handed to a build job is reported, whichever step holds it', () => {
  // Verify artifacts runs no build, but its job does: rule 5 reports the job, rule 2 the step.
  const planted = plant(
    TEXT,
    '      - name: Verify artifacts\n        run: |\n',
    '      - name: Verify artifacts\n        env:\n          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n        run: |\n',
  );
  reported(planted, 'job linux-x86: step "Verify artifacts" holds TAURI_SIGNING_PRIVATE_KEY on a runner that also runs third-party code (step "');
});

test('a third-party step added to the signing job is reported', () => {
  const planted = plant(
    TEXT,
    `      - name: ${SIGN_STEP}\n`,
    `      - name: Install minisign\n        run: sudo apt-get install -y minisign\n\n      - name: ${SIGN_STEP}\n`,
  );
  reported(planted, `job sign-updaters: step "${SIGN_STEP}" holds TAURI_SIGNING_PRIVATE_KEY on a runner that also runs third-party code (step "Install minisign": apt`);
});

test('an artifact downloaded from a job not needed, or from no job at all, is reported', () => {
  const unneeded = plant(TEXT, 'smoke-windows, sign-updaters, macos', 'smoke-windows, macos');
  reported(unneeded, 'downloads "nexus-updater-sigs" from job sign-updaters, which job publish does not need');
  const typo = plant(TEXT, '          pattern: nexus-updater-sigs\n', '          pattern: nexus-updater-sig\n');
  reported(typo, 'downloads "nexus-updater-sig", which no job uploads');
});

test('the signing certificate imported before the builds is reported', () => {
  const start = TEXT.indexOf(`      - name: ${IMPORT}\n`);
  const end = TEXT.indexOf('      - name: Sign the bundled Hamlib binaries\n');
  assert.ok(start > 0 && end > start, 'could not find the import step and the step after it');
  const importStep = TEXT.slice(start, end);
  const planted = plant(
    TEXT.slice(0, start) + TEXT.slice(end),
    '      - name: Build the web UI\n',
    `${importStep}      - name: Build the web UI\n`,
  );
  reported(planted, 'job macos, step "Build the web UI": npm runs while the signing identity');
});

test('the keychain left in place after bundling is reported', () => {
  const planted = plant(TEXT, '          security delete-keychain "$RUNNER_TEMP/nexus-signing.keychain-db"\n', '');
  reported(planted, 'job macos, step "Verify the artifacts": rigctld (built from the Hamlib tarball) runs while the signing identity');
});

test('a stale row in SEEN is reported', () => {
  const planted = plant(TEXT, `      - name: ${SIGN_STEP}\n`, '      - name: Sign the installers\n');
  reported(planted, `SEEN lists TAURI_SIGNING_PRIVATE_KEY for job sign-updaters, step "${SIGN_STEP}", which does not exist`);
});

test('the reader refuses what it cannot place', () => {
  const macos = '    name: macOS (Apple Silicon .dmg)\n    runs-on: macos-14\n';
  const tab = plant(TEXT, macos, `${macos}\tenv: {}\n`);
  assert.throws(() => readWorkflow(tab), /a tab in the indentation/);
  const alias = plant(
    TEXT,
    '          CLUBLOG_API_KEY: ${{ secrets.CLUBLOG_API_KEY }}\n        run: bash scripts/build-windows-cross.sh\n',
    '          CLUBLOG_API_KEY: *clublog\n        run: bash scripts/build-windows-cross.sh\n',
  );
  assert.throws(() => readWorkflow(alias), /a YAML anchor or alias/);
  const flow = plant(TEXT, macos, `${macos}    needs: [linux-x86,\n      pi]\n`);
  assert.throws(() => readWorkflow(flow), /a flow collection that continues/);
});
