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
// BUILD_LINUX=<path> reads a different copy of scripts/build-linux.sh (the pinned-download checks).
const BUILD_LINUX = process.env.BUILD_LINUX || path.join(ROOT, 'scripts', 'build-linux.sh');
// CI_WORKFLOW and DOCKERFILE_PI do the same for ci.yml and scripts/Dockerfile.pi, which the pinned-download
// checks read too: ci.yml gates every push, and Dockerfile.pi builds the Pi packages that ship.
const CI_WORKFLOW = process.env.CI_WORKFLOW || path.join(ROOT, '.github', 'workflows', 'ci.yml');
const DOCKERFILE_PI = process.env.DOCKERFILE_PI || path.join(ROOT, 'scripts', 'Dockerfile.pi');
// STREET_MAPS_WORKFLOW does the same for street-maps.yml, whose bucket job holds a write token for the
// street-map host and runs what it downloads.
const STREET_MAPS_WORKFLOW = process.env.STREET_MAPS_WORKFLOW || path.join(ROOT, '.github', 'workflows', 'street-maps.yml');

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
  // The step that compiles the key in, one per official build (the Linux and Pi rows since #388);
  // COMPILED_IN lets such a step be third-party.
  CLUBLOG_API_KEY: [
    ['linux-x86', 'Build .deb + AppImage'],
    ['windows', 'Cross-build the NSIS installer'],
    ['macos', 'Compile the app'],
    ['pi', 'Build .deb in a debian:${{ matrix.base }} container'],
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
      // A plain scalar ends where ` #` starts a comment (`uses: x@<sha>  # master, 2026-09-12`).
      // The comment is filed as one, so the accounting of secret expressions still sees it.
      const plain = !/^["'[{|>&*]/.test(value) && value.match(/^(.*?)\s+(#.*)$/);
      if (plain) entries.push({ path: ['#'], value: plain[2], line: n + 1 });
      entries.push({ path: pathHere, value: scalar(n, plain ? plain[1] : value), line: n + 1 });
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

// Every `signer sign` in the workflow, and whether it binds the app version. tauri-cli 2.11.5's
// bundler puts the version in the signature's trusted comment, which the signature covers;
// `signer sign` does so only with --app-version. A signature without it is accepted today and
// refused by any client whose updater sets `requireSignedVersion`, so it is a trap that waits on
// one config change.
function unboundSignatures(wf) {
  const calls = [];
  for (const j of wf.jobs.values()) {
    for (const s of j.steps) {
      for (const line of (s?.run ?? '').split('\n')) {
        if (!/^\s*#/.test(line) && /\bsigner\s+sign\b/.test(line)) calls.push({ where: `job ${j.id}, step "${s.label}"`, line: line.trim() });
      }
    }
  }
  return { calls, bare: calls.filter((c) => !/\s--app-version\s+"\$[A-Za-z_]+"\s/.test(c.line)) };
}

test('every updater signature the release makes binds the app version', () => {
  const { calls, bare } = unboundSignatures(WF);
  // The positive control: the NSIS and the AppImage signature are both found.
  assert.equal(calls.length, 2, `expected the NSIS and the AppImage signature, found:\n${calls.map((c) => c.line).join('\n')}`);
  assert.deepEqual(bare.map((c) => `${c.where}: ${c.line}`), []);
});

test('a signature made without the version is reported', () => {
  const planted = plant(TEXT, '--app-version "$ver" "$exe"', '"$exe"');
  const { bare } = unboundSignatures(readWorkflow(planted));
  assert.ok(bare.some((c) => c.line.includes('"$exe"')), `the NSIS signature without --app-version was not reported: ${JSON.stringify(bare)}`);
});

// ---- Pinned downloads --------------------------------------------------------------------------
// Everything the release and CI fetch to build with is pinned to a digest or a commit, or a build can
// change with no commit here: an action by branch, a tool from a `continuous` release, a package at
// whatever version its index serves that day. These checks read what release.yml, ci.yml,
// build-linux.sh and Dockerfile.pi NAME. They cannot see a package manager's choice (apt, apk and
// Homebrew verify what they fetch but float its version), or what a pinned third-party action fetches
// inside itself.
//
// What tauri's AppImage bundler downloads for itself: tauri-bundler 2.9.4 (what tauri-cli 2.11.5
// locks), bundle/linux/appimage/linuxdeploy.rs, `prepare_tools`, each fetched only when absent, plus
// the AppImage runtime its appimage plugin fetches unless LDAI_RUNTIME_FILE hands it one. A new
// tauri-cli can change this list, which is why the tauri-cli pin is checked with it.
const BUNDLER_TOOLS = [
  'AppRun-x86_64',
  'linuxdeploy-x86_64.AppImage',
  'linuxdeploy-plugin-gtk.sh',
  'linuxdeploy-plugin-gstreamer.sh',
  'linuxdeploy-plugin-appimage.AppImage',
  'runtime-x86_64',
];
const MOVING = /\/(?:master|main|HEAD|continuous|latest)\//;
// Downloads that are not something to build with: publish-pi reads back the SHA256SUMS.txt this run
// has just published, to append the Pi sums to it.
const NOT_BUILD_INPUTS = [['release.yml', 'publish-pi', 'Upload to the release + extend SHA256SUMS']];
const FETCH = /\b(?:curl|wget)\b|\bgh\s+release\s+download\b/;
const CHECKSUM = /\b(?:sha256sum|sha512sum)\b(?:\s+--?[a-z-]+)*\s+-c\b|\bshasum\s+-a\s+(?:256|512)\b(?:\s+--?[a-z-]+)*\s+-c\b/;

// The rules every workflow is held to: an action from outside GitHub's own at a commit, every fetch
// checked in its own step, pip by hash, cargo installs exact and locked, npm only from a lockfile.
function unpinnedInWorkflow(wf, file) {
  const v = [];
  for (const j of wf.jobs.values()) {
    for (const s of j.steps) {
      if (!s) continue;
      if (s.uses && !/^actions\//.test(s.uses) && !/@[0-9a-f]{40}$/.test(s.uses)) {
        v.push(`${file}: job ${j.id}: \`${s.uses}\` names a branch or a tag, not a commit`);
      }
      const code = (s.run ?? '').split('\n').filter((l) => !/^\s*#/.test(l)).join('\n');
      const input = !NOT_BUILD_INPUTS.some(([f, jid, name]) => f === file && jid === j.id && name === s.name);
      if (input && FETCH.test(code) && !CHECKSUM.test(code)) v.push(`${file}: job ${j.id}, step "${s.label}": downloads with no checksum check in the same step`);
      if (/\bpip3?\s+install\b/.test(code) && !/--require-hashes\b/.test(code)) v.push(`${file}: job ${j.id}, step "${s.label}": pip installs without --require-hashes`);
      for (const m of code.matchAll(/\bcargo\s+install\b[^\n]*/g)) {
        if (!/--version\s+"=[0-9.]+"/.test(m[0]) || !/--locked\b/.test(m[0])) v.push(`${file}: job ${j.id}, step "${s.label}": \`${m[0].trim()}\` is not an exact, --locked version`);
      }
      if (/\bnpm\s+(?:install|i|add)\b/.test(code)) v.push(`${file}: job ${j.id}, step "${s.label}": npm install resolves afresh; use npm ci`);
    }
  }
  return v;
}

// A Dockerfile's instructions, continuation lines joined and comment lines dropped, and its parser
// directives (the `# key=value` lines at the top). A RUN heredoc and an escape directive are refused
// rather than read: a fetch inside a heredoc would be invisible here, and another escape character
// would join the wrong lines.
function dockerInstructions(text, file) {
  const out = [];
  const directives = new Map();
  let top = true;
  let cur = null;
  for (const [i, raw] of text.split('\n').entries()) {
    const directive = top && raw.match(/^#\s*([A-Za-z]+)\s*=\s*(\S.*?)\s*$/);
    if (directive) {
      if (directive[1].toLowerCase() === 'escape') throw new Error(`${file}:${i + 1}: an escape directive — this reader does not model it`);
      directives.set(directive[1].toLowerCase(), { value: directive[2], line: i + 1 });
      continue;
    }
    top = false;
    if (/^\s*(?:#.*)?$/.test(raw)) continue;
    const cont = /\\\s*$/.test(raw);
    const line = raw.replace(/\\\s*$/, '').trim();
    if (cur) {
      cur.args += ` ${line}`;
    } else {
      const m = line.match(/^([A-Za-z]+)\s+(.*)$/);
      if (!m) throw new Error(`${file}:${i + 1}: a line that is not an instruction — this reader does not model it\n  ${raw}`);
      cur = { op: m[1].toUpperCase(), args: m[2], line: i + 1 };
    }
    if (!cont) {
      if (cur.op === 'RUN' && /<<-?\s*["']?[A-Za-z_]+/.test(cur.args)) throw new Error(`${file}:${cur.line}: a RUN heredoc — this reader does not model it`);
      out.push(cur);
      cur = null;
    }
  }
  out.directives = directives;
  return out;
}

// The rules a Dockerfile is held to: the frontend (`# syntax=`) and every base image by digest (or a
// stage of this file that is), no download piped into a shell, no RUN that downloads without a
// checksum check in the same RUN, npm packages at exact versions. A download is curl or wget where a
// command starts, so a package list that names them (`apt-get install curl`) is not one.
const DOCKER_FETCH = /(?:^|&&|\|\||[;|(`])\s*(?:sudo\s+)?(?:curl|wget)\b/;
function unpinnedInDockerfile(text, file) {
  const v = [];
  const ins = dockerInstructions(text, file);
  const syntax = ins.directives.get('syntax');
  if (syntax && !/@sha256:[0-9a-f]{64}$/.test(syntax.value)) v.push(`${file}:${syntax.line}: the syntax directive names ${syntax.value}, a tag, not a digest`);
  const argDefaults = new Map();
  const stages = new Set();
  for (const d of ins) {
    if (d.op === 'ARG' && !stages.size) {
      const m = d.args.match(/^(\w+)=(\S+)$/);
      if (m) argDefaults.set(m[1], m[2]);
    }
    if (d.op === 'FROM') {
      const m = d.args.match(/^(?:--platform=\S+\s+)?(\S+)(?:\s+AS\s+(\S+))?$/i);
      if (!m) throw new Error(`${file}:${d.line}: a FROM this reader cannot read\n  FROM ${d.args}`);
      const [, image, name] = m;
      // An image named through an ARG must, with the ARG's default, name a stage defined above it.
      const resolved = image.replace(/\$\{(\w+)\}|\$(\w+)/g, (_, a, b) => argDefaults.get(a ?? b) ?? '\0');
      if (image !== resolved) {
        if (!stages.has(resolved)) v.push(`${file}:${d.line}: FROM ${image} does not name a stage of this file (it resolves to ${resolved.replace('\0', '<no default>')})`);
      } else if (image !== 'scratch' && !stages.has(image) && !/@sha256:[0-9a-f]{64}$/.test(image)) {
        v.push(`${file}:${d.line}: FROM ${image} names a tag, not a digest`);
      }
      if (name) stages.add(name);
    }
    if (d.op === 'RUN') {
      if (/\b(?:curl|wget)\b[^|;&]*\|\s*(?:sudo\s+)?(?:ba|da|z)?sh\b/.test(d.args)) v.push(`${file}:${d.line}: pipes a download into a shell`);
      if (DOCKER_FETCH.test(d.args) && !CHECKSUM.test(d.args)) v.push(`${file}:${d.line}: downloads with no checksum check in the same RUN`);
      for (const m of d.args.matchAll(/\bnpm\s+(?:install|i|add)\b([^&;|]*)/g)) {
        for (const spec of m[1].trim().split(/\s+/).filter((t) => t && !t.startsWith('-'))) {
          if (!/^(?:@[^/\s]+\/)?[^@\s]+@\d+\.\d+\.\d+$/.test(spec)) v.push(`${file}:${d.line}: npm installs ${spec}, not an exact version`);
        }
      }
    }
  }
  return v;
}

function unpinnedDownloads(wf, script) {
  const v = unpinnedInWorkflow(wf, 'release.yml');
  // build-linux.sh's pins: `pin_tool <file> <url> <sha256> [<sha256>]`, continuation lines joined.
  const calls = script.replace(/\\\n\s*/g, ' ').split('\n').map((l) => l.trim()).filter((l) => /^pin_tool\s/.test(l)).map((l) => l.split(/\s+/));
  for (const tool of BUNDLER_TOOLS) {
    const c = calls.find((a) => a[1] === tool);
    if (!c) v.push(`build-linux.sh: ${tool} is not pinned, so tauri's bundler fetches it unverified`);
    else if (MOVING.test(c[2] ?? '')) v.push(`build-linux.sh: ${tool} is fetched from a moving ref (${c[2]})`);
    else if (!/^[0-9a-f]{64}$/.test(c[3] ?? '')) v.push(`build-linux.sh: ${tool} has no sha256 pin`);
  }
  if (!/^\s*export LDAI_RUNTIME_FILE="\$tauri_tools\/runtime-x86_64"$/m.test(script)) {
    v.push('build-linux.sh: the pinned runtime is not handed to the appimage plugin (LDAI_RUNTIME_FILE), so appimagetool downloads one');
  }
  if (!/cargo install tauri-cli --version "=2\.11\.5" --locked/.test(script)) {
    v.push("build-linux.sh: tauri-cli is no longer pinned at 2.11.5; re-read that bundler's prepare_tools and update BUNDLER_TOOLS in this test");
  }
  // A pin the artifact does not show is a guess: the Linux Verify step checks the shipped runtime.
  const verify = wf.jobs.get('linux-x86')?.steps.find((s) => s?.name === 'Verify artifacts');
  if (!/^\s*python3 - "\$app" "\$HOME\/\.cache\/tauri\/runtime-x86_64" <<'PY'$/m.test(verify?.run ?? '')) {
    v.push('release.yml: job linux-x86, step "Verify artifacts": does not check that the shipped AppImage carries the pinned runtime');
  }
  return v;
}

const SCRIPT = fs.readFileSync(BUILD_LINUX, 'utf8');
const CI_TEXT = fs.readFileSync(CI_WORKFLOW, 'utf8');
const CI_WF = readWorkflow(CI_TEXT, path.basename(CI_WORKFLOW));
const DOCKERFILE = fs.readFileSync(DOCKERFILE_PI, 'utf8');
const STREET_TEXT = fs.readFileSync(STREET_MAPS_WORKFLOW, 'utf8');
const STREET_WF = readWorkflow(STREET_TEXT, path.basename(STREET_MAPS_WORKFLOW));

test('the readers account for every step of ci.yml and every instruction of Dockerfile.pi', () => {
  // The positive controls for the two other files: a reader that dropped a step or an instruction
  // would find nothing unpinned in it.
  const raw = [...CI_TEXT.matchAll(EXPR)].filter(([, e]) => MENTIONS.test(e)).length;
  assert.equal(CI_WF.refs.length, raw, 'the reader filed a different number of secret expressions than ci.yml holds');
  const steps = [...CI_WF.jobs.values()].reduce((n, j) => n + j.steps.filter(Boolean).length, 0);
  assert.equal(steps, CI_TEXT.match(/^ {6}- [A-Za-z0-9_-]+:/gm)?.length ?? 0, 'the reader found a different number of steps than ci.yml has');
  const ins = dockerInstructions(DOCKERFILE, 'Dockerfile.pi');
  const lines = DOCKERFILE.split('\n');
  const starts = lines.filter((l, i) => /^[A-Za-z]+\s/.test(l) && !/\\\s*$/.test(lines[i - 1] ?? '')).length;
  assert.equal(ins.length, starts, 'the Dockerfile reader found a different number of instructions than Dockerfile.pi has');
  assert.ok(ins.filter((d) => d.op === 'RUN' && /\bcurl\b/.test(d.args)).length >= 2, 'the Dockerfile reader sees no RUN that downloads; it is reading the wrong file');
});

test('everything the release and CI fetch to build with is pinned', () => {
  const v = [
    ...unpinnedDownloads(WF, SCRIPT),
    ...unpinnedInWorkflow(CI_WF, 'ci.yml'),
    ...unpinnedInWorkflow(STREET_WF, 'street-maps.yml'),
    ...unpinnedInDockerfile(DOCKERFILE, 'Dockerfile.pi'),
  ];
  assert.deepEqual(v, [], `\n${v.join('\n')}\n`);
});

test('an unpinned download planted in street-maps.yml is reported', () => {
  const steps = [...STREET_WF.jobs.values()].reduce((n, j) => n + j.steps.filter(Boolean).length, 0);
  assert.equal(steps, STREET_TEXT.match(/^ {6}- [A-Za-z0-9_-]+:/gm)?.length ?? 0, 'the reader found a different number of steps than street-maps.yml has');
  const sha = '02cb101ec7c40f2c49e1d9714d64511d8e1b74de';
  const cases = [
    [plant(STREET_TEXT, `dtolnay/rust-toolchain@${sha}  # master, 2026-09-12`, 'dtolnay/rust-toolchain@master'), 'street-maps.yml: job plan: `dtolnay/rust-toolchain@master` names a branch or a tag'],
    // Two jobs install rclone (the plan job's tests and the bucket job), with the same checksum line, so each plant
    // carries its own job's context: removing the check from either install must be reported for THAT job.
    [plant(STREET_TEXT, "      # The tests copy a build with the rclone the bucket job runs, into a local folder.\n      - name: Install rclone 1.75.1, checked against its published SHA-256\n        run: |\n          curl -fsSLo \"$RUNNER_TEMP/rclone.zip\" https://downloads.rclone.org/v1.75.1/rclone-v1.75.1-linux-amd64.zip\n          echo \"982b5aa772841168f8e380f139e9e787b2a105403e32b94da8676a0e1c0a13ab  $RUNNER_TEMP/rclone.zip\" | sha256sum -c -\n", "      # The tests copy a build with the rclone the bucket job runs, into a local folder.\n      - name: Install rclone 1.75.1, checked against its published SHA-256\n        run: |\n          curl -fsSLo \"$RUNNER_TEMP/rclone.zip\" https://downloads.rclone.org/v1.75.1/rclone-v1.75.1-linux-amd64.zip\n"), "street-maps.yml: job plan, step \"Install rclone 1.75.1, checked against its published SHA-256\": downloads with no checksum check"],
    [plant(STREET_TEXT, "          path: out\n\n      - name: Install rclone 1.75.1, checked against its published SHA-256\n        run: |\n          curl -fsSLo \"$RUNNER_TEMP/rclone.zip\" https://downloads.rclone.org/v1.75.1/rclone-v1.75.1-linux-amd64.zip\n          echo \"982b5aa772841168f8e380f139e9e787b2a105403e32b94da8676a0e1c0a13ab  $RUNNER_TEMP/rclone.zip\" | sha256sum -c -\n", "          path: out\n\n      - name: Install rclone 1.75.1, checked against its published SHA-256\n        run: |\n          curl -fsSLo \"$RUNNER_TEMP/rclone.zip\" https://downloads.rclone.org/v1.75.1/rclone-v1.75.1-linux-amd64.zip\n"), "street-maps.yml: job bucket, step \"Install rclone 1.75.1, checked against its published SHA-256\": downloads with no checksum check"],
  ];
  for (const [text, needle] of cases) {
    const v = unpinnedInWorkflow(readWorkflow(text, 'street-maps.yml'), 'street-maps.yml');
    assert.ok(v.some((x) => x.includes(needle)), `a planted unpinned download was not reported. Expected:\n  ${needle}\ngot:\n  ${v.join('\n  ') || '(nothing)'}`);
  }
});

test('an unpinned download planted in the workflow or build-linux.sh is reported', () => {
  const sha = '02cb101ec7c40f2c49e1d9714d64511d8e1b74de';
  const cases = [
    [plant(TEXT, `dtolnay/rust-toolchain@${sha}  # master, 2026-09-12\n        with:\n          toolchain: 1.93.1\n\n      # Pinned as every other`, `dtolnay/rust-toolchain@master\n        with:\n          toolchain: 1.93.1\n\n      # Pinned as every other`), SCRIPT, 'names a branch or a tag, not a commit'],
    [plant(TEXT, '          echo "$EC_SHA256  /tmp/ec.zip" | sha256sum -c -\n', ''), SCRIPT, 'step "Validate the EPUB (W3C epubcheck — the shipped download must be valid)": downloads with no checksum check'],
    [plant(TEXT, ' --require-hashes -r "$RUNNER_TEMP/manual-requirements.txt"', ' -r "$RUNNER_TEMP/manual-requirements.txt"'), SCRIPT, 'pip installs without --require-hashes'],
    [TEXT, plant(SCRIPT, 'linuxdeploy-plugin-gstreamer/2a2e67491c32995a3f279ad0ecbe77abd512b42a/', 'linuxdeploy-plugin-gstreamer/master/'), 'linuxdeploy-plugin-gstreamer.sh is fetched from a moving ref'],
    [TEXT, plant(SCRIPT, '  export LDAI_RUNTIME_FILE="$tauri_tools/runtime-x86_64"\n', ''), 'the pinned runtime is not handed to the appimage plugin'],
    [plant(TEXT, `"$HOME/.cache/tauri/runtime-x86_64" <<'PY'`, `"$HOME/.cache/tauri/runtime" <<'PY'`), SCRIPT, 'does not check that the shipped AppImage carries the pinned runtime'],
  ];
  for (const [text, script, needle] of cases) {
    const v = unpinnedDownloads(readWorkflow(text), script);
    assert.ok(v.some((x) => x.includes(needle)), `a planted unpinned download was not reported. Expected:\n  ${needle}\ngot:\n  ${v.join('\n  ') || '(nothing)'}`);
  }
});

test('an unpinned download planted in ci.yml is reported', () => {
  const sha = '02cb101ec7c40f2c49e1d9714d64511d8e1b74de';
  const deny = '3c6349835b2b7b196a839186cb8b78e02f7b5f25';
  const cases = [
    [plant(CI_TEXT, `dtolnay/rust-toolchain@${sha}  # master, 2026-09-12\n        with:\n          toolchain: 1.91.0`, 'dtolnay/rust-toolchain@1.91.0\n        with:\n          toolchain: 1.91.0'), 'ci.yml: job msrv: `dtolnay/rust-toolchain@1.91.0` names a branch or a tag'],
    [plant(CI_TEXT, `cargo-deny-action@${deny}  # v2, 2026-07-13\n        with:\n          command: check advisories bans sources licenses\n          manifest-path:`, 'cargo-deny-action@v2\n        with:\n          command: check advisories bans sources licenses\n          manifest-path:'), 'ci.yml: job deny: `EmbarkStudios/cargo-deny-action@v2` names a branch or a tag'],
    [plant(CI_TEXT, ' --require-hashes -r /dev/stdin', ' -r /dev/stdin'), 'ci.yml: job manual-epub, step "Tools (pandoc + pillow; Java is preinstalled)": pip installs without --require-hashes'],
    [plant(CI_TEXT, '          echo "$EC_SHA256  /tmp/ec.zip" | sha256sum -c -\n', ''), 'ci.yml: job manual-epub, step "Validate with W3C epubcheck": downloads with no checksum check'],
    [plant(CI_TEXT, 'cargo install tauri-driver --version "=2.1.0" --locked', 'cargo install tauri-driver --locked'), 'ci.yml: job e2e-linux, step "Install tauri-driver": `cargo install tauri-driver --locked` is not an exact, --locked version'],
  ];
  for (const [text, needle] of cases) {
    const v = unpinnedInWorkflow(readWorkflow(text, 'ci.yml'), 'ci.yml');
    assert.ok(v.some((x) => x.includes(needle)), `a planted unpinned download was not reported. Expected:\n  ${needle}\ngot:\n  ${v.join('\n  ') || '(nothing)'}`);
  }
});

test('an unpinned download planted in Dockerfile.pi is reported', () => {
  const cases = [
    [plant(DOCKERFILE, 'debian:trixie@sha256:9cc080028c43b27d2074d63a5f9caf7166d731494965616c1a6d2827a004585c', 'debian:trixie'), 'FROM debian:trixie names a tag, not a digest'],
    [plant(DOCKERFILE, 'FROM debian-${BASE} AS build', 'FROM debian:${BASE} AS build'), 'FROM debian:${BASE} does not name a stage of this file (it resolves to debian:bookworm)'],
    [plant(DOCKERFILE, 'ENV DEBIAN_FRONTEND=noninteractive\n', 'ENV DEBIAN_FRONTEND=noninteractive\nRUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash -\n'), 'pipes a download into a shell'],
    [plant(DOCKERFILE, '    && echo "15f6e4ce9f583b929c996c91562bad6d4454f3281de858b02cdfdef615fac433  /tmp/rustup-init" | sha256sum -c - \\\n', ''), 'downloads with no checksum check in the same RUN'],
    [plant(DOCKERFILE, 'npm install -g npm@11.21.0', 'npm install -g npm@11'), 'npm installs npm@11, not an exact version'],
    [plant(DOCKERFILE, '# syntax=docker/dockerfile:1.27@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e\n', '# syntax=docker/dockerfile:1\n'), 'Dockerfile.pi:1: the syntax directive names docker/dockerfile:1, a tag, not a digest'],
  ];
  for (const [text, needle] of cases) {
    const v = unpinnedInDockerfile(text, 'Dockerfile.pi');
    assert.ok(v.some((x) => x.includes(needle)), `a planted unpinned download was not reported. Expected:\n  ${needle}\ngot:\n  ${v.join('\n  ') || '(nothing)'}`);
  }
});

test('the Dockerfile reader refuses what it cannot place', () => {
  const heredoc = plant(DOCKERFILE, 'ENV DEBIAN_FRONTEND=noninteractive\n', 'ENV DEBIAN_FRONTEND=noninteractive\nRUN <<EOF\ncurl -fsSL https://example.invalid/x | sh\nEOF\n');
  assert.throws(() => unpinnedInDockerfile(heredoc, 'Dockerfile.pi'), /a RUN heredoc/);
  const escape = plant(DOCKERFILE, '# syntax=', '# escape=`\n# syntax=');
  assert.throws(() => unpinnedInDockerfile(escape, 'Dockerfile.pi'), /an escape directive/);
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

// #388. A container build does not pass its environment on to the build inside it: the Pi step's
// `--secret id=clublog_api_key,env=CLUBLOG_API_KEY` only offers the key to BuildKit, and the RUN
// that compiles Nexus has to mount it. Without the mount the build stays green and the Pi packages
// ship with no ClubLog key, as they did before it was wired. The YAML checks above cannot see this.
test('the Pi container build hands the ClubLog key to the RUN that compiles it in', () => {
  const step = findStep(WF, 'pi', 'Build .deb in a debian:${{ matrix.base }} container');
  assert.match(step.run, /--secret id=clublog_api_key,env=CLUBLOG_API_KEY(?:\s|$)/, 'the Pi build step does not offer the ClubLog key to BuildKit');
  const compile = dockerInstructions(DOCKERFILE, 'Dockerfile.pi').filter((d) => d.op === 'RUN' && /\.\/scripts\/build-linux\.sh\b/.test(d.args));
  assert.equal(compile.length, 1, 'expected one RUN in Dockerfile.pi to run build-linux.sh');
  assert.match(compile[0].args, /^(?:--\S+\s+)*--mount=type=secret,id=clublog_api_key,env=CLUBLOG_API_KEY\s/, 'the RUN that compiles Nexus does not mount the ClubLog key');
});
