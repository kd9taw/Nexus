#!/usr/bin/env node
// The documentation pass that closes a release. Run it AFTER the release is live.
//
// WHY THIS EXISTS. Every release moves code and leaves the doc set behind, and nothing has ever
// gone red about it. 1.3.0 proved it three separate ways in one release:
//
//   1. Settings went from eight tabs to nine ("Modes" split into Phone/CW/Digital, "Rig Control"
//      renamed "Rig & CAT", Frequencies folded into Digital). Roughly thirty-four doc files still
//      described the old panel, and every one was found BY HAND, after the release had shipped.
//   2. Three manual screenshots still showed the eight-tab panel. They had not been recaptured
//      since 2026-08-04; the panel changed on 2026-08-12.
//   3. The CHANGELOG credited "(#2, #8)" as fixed in 1.3.0. Both issues were, and are, OPEN.
//      That one reached the public.
//
// And a fourth, found while writing this: `scripts/gen-wiki.mjs` has THROWN on every invocation
// since a6097c92 (2026-08-09), because docs/install.md stopped containing the text one of its
// required replacements is keyed on. Three releases shipped over the top of a generator that
// could not run, because nothing ran it. That is the shape of the whole problem — the mechanism
// existed and was never wired to anything that fires.
//
// So this is a RUNNER, not a new pile of logic. Where a checker already exists it is invoked;
// where the answer is in git it is computed; where only a human can judge it is printed with the
// specific file, line and commit rather than as a checklist item saying "review the docs".
//
// *** THIS TOOL IS READ-ONLY. It writes no file, stages nothing, and every git call it makes  ***
// *** is a query (log / status / tag). Other agents work in this checkout — see CLAUDE.md's   ***
// *** working protocol. It reports; you fix.                                                  ***
//
// TWO SEVERITIES, and the difference is deliberate:
//   FAIL   — mechanically wrong. Exits 1. Fix it before you call the release closed.
//   REVIEW — a human has to look. Exits 0, printed last, above the manual steps. These do NOT
//            fail the run, because a check that is red every single release is a check that gets
//            switched off, and then the FAILs go with it.
//
// Run:   node scripts/release-docs.mjs
//        node scripts/release-docs.mjs --version 1.3.0   (default: src-tauri/tauri.conf.json)
//        node scripts/release-docs.mjs --no-tests        (skip the vitest doc gates)
//
// The order matters and is not arbitrary — see .claude/skills/release-docs/SKILL.md.

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const DOCS = path.join(ROOT, 'docs')
const rel = (abs) => path.relative(ROOT, abs).replace(/\\/g, '/')

const argv = process.argv.slice(2)
const flag = (n) => argv.includes(n)
const opt = (n) => {
  const i = argv.indexOf(n)
  return i >= 0 ? argv[i + 1] : undefined
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

const results = []
const report = (name, severity, lines, note) => results.push({ name, severity, lines, note })
const OK = (name, note) => report(name, 'ok', [], note)
const FAIL = (name, lines, note) => report(name, 'fail', lines, note)
const REVIEW = (name, lines, note) => report(name, 'review', lines, note)
const SKIP = (name, note) => report(name, 'skip', [], note)

// ---------------------------------------------------------------------------
// git — queries only
// ---------------------------------------------------------------------------

const git = (...args) =>
  execFileSync('git', ['-C', ROOT, ...args], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }).trim()

/** `{ sha, at, date, subject }` of the last commit to touch `p`, or null if git has never seen it. */
function lastCommit(p) {
  const out = git('log', '-1', '--format=%h|%ct|%cs|%s', '--', p)
  if (!out) return null
  // A subject may contain `|`, so it takes everything after the third field rather than the fourth.
  const [sha, ct, date, ...subject] = out.split('|')
  return { sha, at: Number(ct), date, subject: subject.join('|') }
}

/** How many commits touched `p` strictly after unix time `since`. */
const commitsSince = (p, since) =>
  git('log', `--since=${since + 1}`, '--format=%h', '--', p).split('\n').filter(Boolean).length

/** True when the path has uncommitted changes — i.e. someone is fixing it right now. */
const isDirty = (p) => git('status', '--porcelain', '--', p).length > 0

// ---------------------------------------------------------------------------
// The released version
// ---------------------------------------------------------------------------

const VERSION =
  opt('--version') ?? JSON.parse(readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).version

// ---------------------------------------------------------------------------
// 1. The generators — the mechanised half, which today nothing invokes
// ---------------------------------------------------------------------------

function runNode(script, args) {
  try {
    const out = execFileSync('node', [path.join(ROOT, 'scripts', script), ...args], {
      cwd: ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return { code: 0, out }
  } catch (e) {
    return { code: e.status ?? 1, out: `${e.stdout ?? ''}${e.stderr ?? ''}` }
  }
}

function checkGenerators() {
  for (const [script, what] of [
    ['gen-settings-reference.mjs', 'docs/guide/settings-reference.md'],
    ['gen-wiki.mjs', 'docs/launch/wiki/{FAQ,Install,Quick-Start}.md'],
  ]) {
    const r = runNode(script, ['--check'])
    const name = `${script} --check`
    if (r.code === 0) {
      OK(name, `${what} is current`)
    } else {
      FAIL(name, r.out.split('\n').filter(Boolean).slice(-14),
        `${what} is stale or the generator cannot run. Re-run it WITHOUT --check, or fix the ` +
          `rule it is complaining about, then commit what changed.`)
    }
  }
}

// ---------------------------------------------------------------------------
// 2. The doc gates that live in vitest
// ---------------------------------------------------------------------------

const DOC_TESTS = ['src/docs-match-code.test.ts', 'src/docs-settings-pointers.test.ts']

function checkDocGates() {
  if (flag('--no-tests')) return SKIP('doc gates (vitest)', '--no-tests was passed')
  let out = ''
  let code = 0
  try {
    out = execFileSync('npm', ['--prefix', path.join(ROOT, 'ui'), 'exec', '--', 'vitest', 'run', ...DOC_TESTS], {
      cwd: ROOT,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch (e) {
    code = e.status ?? 1
    out = `${e.stdout ?? ''}${e.stderr ?? ''}`
  }
  const name = `vitest ${DOC_TESTS.join(' ')}`
  if (code === 0) {
    OK(name, 'the doc-vs-code guards are green')
  } else {
    // CI runs these too, so a red here means the tree moved since the last green run.
    FAIL(name, out.split('\n').filter((l) => /FAIL|×|AssertionError|points at|says /.test(l)).slice(0, 24),
      `Run it yourself for the full output: npm --prefix ui exec -- vitest run ${DOC_TESTS.join(' ')}`)
  }
}

// ---------------------------------------------------------------------------
// 3. Screenshots — against git history, never against mtime
// ---------------------------------------------------------------------------
//
// ⚠️ FILE MTIME IS MEANINGLESS HERE. `git checkout` and `git worktree add` rewrite it, so every
// image in a fresh clone looks like it was captured this morning. The only honest question is
// which COMMIT last touched the image versus which commit last touched the code it depicts.
//
// The mapping has to be declared, because nothing in an image says what it shows. An image with
// no entry FAILS rather than being skipped: an unmapped capture is a check quietly shrinking,
// which is how the gap this whole script exists for opened in the first place.

const SHOTS = {
  'settings-radio.webp': ['ui/src/components/SettingsPanel.tsx', 'ui/src/settings/registry.ts'],
  'settings-modes.webp': ['ui/src/components/SettingsPanel.tsx', 'ui/src/settings/registry.ts'],
  'settings-contesting.webp': ['ui/src/components/SettingsPanel.tsx', 'ui/src/settings/registry.ts'],
  'cw-cockpit.webp': ['ui/src/components/CwCockpit.tsx'],
  'phone-cockpit.webp': ['ui/src/components/PhoneCockpit.tsx'],
  'rtty-cockpit.webp': ['ui/src/components/RttyCockpit.tsx'],
  'sstv.webp': ['ui/src/components/SstvView.tsx'],
  'operate-classic.webp': ['ui/src/components/OperateCockpit.tsx', 'ui/src/components/OperateDecodes.tsx'],
  'operate-roster.webp': ['ui/src/components/OperateCockpit.tsx', 'ui/src/components/OperateRoster.tsx'],
  'awards-official.webp': ['ui/src/components/AwardsView.tsx'],
  'satellites-console.webp': ['ui/src/components/SatellitesView.tsx'],
}

function checkScreenshots() {
  const dir = path.join(DOCS, 'img', 'manual')
  if (!existsSync(dir)) return SKIP('manual screenshots', `${rel(dir)} does not exist`)
  const images = readdirSync(dir).filter((f) => !statSync(path.join(dir, f)).isDirectory())

  // Positive control on the mapping itself, in both directions.
  const unmapped = images.filter((f) => !SHOTS[f])
  const phantom = Object.keys(SHOTS).filter((f) => !images.includes(f))
  const badOwners = Object.entries(SHOTS).flatMap(([img, owners]) =>
    owners.filter((o) => !existsSync(path.join(ROOT, o))).map((o) => `${img} -> ${o} does not exist`))
  const mappingProblems = [
    ...unmapped.map((f) => `docs/img/manual/${f} has no SHOTS entry — it is being checked by nobody`),
    ...phantom.map((f) => `SHOTS names ${f}, which is not in docs/img/manual/ — a dead entry`),
    ...badOwners,
  ]
  if (mappingProblems.length) {
    FAIL('screenshot mapping', mappingProblems,
      'Fix the SHOTS table in scripts/release-docs.mjs. Every image must name the source it shows.')
  } else {
    OK('screenshot mapping', `${images.length} images, all mapped to source that exists`)
  }

  // RANKED, not just listed. A commit that touches every cockpit at once (1db7d051 did) puts
  // eight images on this list at equal weight, and a flat list of eight is a list nobody reads.
  // The commit COUNT since the capture is the closest honest proxy for how much of the frame has
  // moved, so it goes first and the list is sorted by it.
  const stale = []
  const pending = []
  for (const [img, owners] of Object.entries(SHOTS)) {
    const imgPath = `docs/img/manual/${img}`
    if (!existsSync(path.join(ROOT, imgPath))) continue // already reported as phantom
    if (isDirty(imgPath)) {
      pending.push(`${img} — recaptured but not committed yet`)
      continue
    }
    const shot = lastCommit(imgPath)
    if (!shot) continue
    const newer = owners
      .map((o) => ({ o, c: lastCommit(o), n: commitsSince(o, shot.at) }))
      .filter(({ c }) => c && c.at > shot.at)
      .sort((a, b) => b.n - a.n)
    if (!newer.length) continue
    const total = newer.reduce((s, x) => s + x.n, 0)
    stale.push({
      total,
      line:
        `${String(total).padStart(3)} commits · ${img} captured ${shot.date} (${shot.sha}) · ` +
        newer
          .slice(0, 2)
          .map(({ o, c, n }) => `${path.basename(o)} +${n}, latest ${c.date} ${c.sha} "${c.subject}"`)
          .join(' · ') +
        (newer.length > 2 ? ` · +${newer.length - 2} more owners` : ''),
    })
  }
  if (pending.length) OK('screenshots pending commit', pending.join(' · '))
  if (stale.length) {
    REVIEW('screenshots older than the UI they show',
      stale.sort((a, b) => b.total - a.total).map((s) => s.line),
      'Ranked by how many commits landed on the source since the capture — work down, not across. ' +
        'A source change is not proof the FRAME changed, so open the image before re-shooting. ' +
        'Recapture at a 1920x1080 LOGICAL window (the app auto-zoom resolves to 100% there), run ' +
        'scripts/build-manual-images.py, and REWORD THE ALT TEXT — it describes the picture, so a ' +
        'new picture under the old sentence is a new lie.')
  } else if (!mappingProblems.length) {
    OK('screenshot freshness', 'every capture is newer than the source it shows')
  }
}

// ---------------------------------------------------------------------------
// 4. The release's own two doc deliverables
// ---------------------------------------------------------------------------

function checkReleaseDocs() {
  const notes = path.join(DOCS, `RELEASE_NOTES-${VERSION}.md`)
  if (existsSync(notes)) {
    OK(`docs/RELEASE_NOTES-${VERSION}.md`, 'present')
  } else {
    FAIL(`docs/RELEASE_NOTES-${VERSION}.md`, [`${rel(notes)} does not exist`],
      "Write it before tagging: without it the update feed's `notes` field degrades to whatever " +
        'the GitHub release body happens to say.')
  }

  const changelog = readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8')
  const stamped = new RegExp(`^## \\[${VERSION.replace(/\./g, '\\.')}\\]\\s+—\\s+\\d{4}-\\d{2}-\\d{2}`, 'm')
  if (stamped.test(changelog)) {
    OK('CHANGELOG version section', `## [${VERSION}] is stamped with a date`)
  } else {
    FAIL('CHANGELOG version section', [`no dated "## [${VERSION}] — YYYY-MM-DD" heading in CHANGELOG.md`],
      'scripts/release-prep stamps this. If it is still [Unreleased], the release was cut by hand.')
  }
}

// ---------------------------------------------------------------------------
// 5. Issue credits — the failure that reached the public
// ---------------------------------------------------------------------------
//
// "(#2, #8)" was published in the 1.3.0 CHANGELOG as fixed. Both were open then and are open
// now. This is the only check here that needs the network, so it degrades to a loud SKIP rather
// than a silent pass when `gh` cannot answer.

function versionSection(changelog, version) {
  const start = changelog.search(new RegExp(`^## \\[${version.replace(/\./g, '\\.')}\\]`, 'm'))
  if (start < 0) return null
  const after = changelog.slice(start + 1)
  const end = after.search(/^## \[/m)
  return end < 0 ? after : after.slice(0, end)
}

function checkIssueCredits() {
  const section = versionSection(readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8'), VERSION)
  if (section == null) return SKIP('CHANGELOG issue credits', `no [${VERSION}] section to read`)
  const cited = [...new Set([...section.matchAll(/#(\d{1,5})\b/g)].map((m) => m[1]))]
  if (!cited.length) return OK('CHANGELOG issue credits', `the [${VERSION}] section cites no issues`)

  let json
  try {
    json = execFileSync(
      'gh',
      ['issue', 'list', '--repo', 'kd9taw/Nexus', '--state', 'all', '--limit', '400', '--json', 'number,state,title'],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
    )
  } catch (e) {
    return SKIP('CHANGELOG issue credits',
      `gh could not read the issue list (${String(e.message).split('\n')[0]}). The [${VERSION}] ` +
        `section cites #${cited.join(', #')} — verify each is CLOSED by hand.`)
  }
  const byNumber = new Map(JSON.parse(json).map((i) => [String(i.number), i]))
  const open = cited
    .map((n) => byNumber.get(n))
    .filter((i) => i && i.state !== 'CLOSED')
    .map((i) => `#${i.number} "${i.title}" is ${i.state} — the CHANGELOG credits it as done in ${VERSION}`)
  const missing = cited.filter((n) => !byNumber.has(n)).map((n) => `#${n} is not an issue in kd9taw/Nexus`)

  if (open.length || missing.length) {
    FAIL('CHANGELOG issue credits', [...open, ...missing],
      'A released section is immutable history, so do not quietly rewrite it: correct it in the ' +
        'NEXT version section and, if the issue is genuinely not fixed, say so on the issue. ' +
        'Never close an issue because the CHANGELOG claimed it.')
  } else {
    OK('CHANGELOG issue credits', `#${cited.join(', #')} — all closed`)
  }
}

// ---------------------------------------------------------------------------
// 6. Prose that still names an older release
// ---------------------------------------------------------------------------
//
// Narrow on purpose. Only strings that match a version Nexus HAS ACTUALLY TAGGED count, which is
// what keeps `127.0.0.1` and "ADIF 3.1.4" out of the report. Release notes are excluded: they are
// a point-in-time record, and so is a sentence like "fatal to Tempo before 0.19.6" — which is why
// this is REVIEW and not FAIL. A human reads the line and decides.

function docFiles() {
  const out = []
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const abs = path.join(dir, name)
      if (statSync(abs).isDirectory()) walk(abs)
      else if (name.endsWith('.md') && !/RELEASE_NOTES-/.test(name)) out.push(abs)
    }
  }
  walk(DOCS)
  out.push(path.join(ROOT, 'README.md'))
  return out.sort()
}

function checkVersionProse() {
  const tagged = new Set(git('tag', '--list', 'v*').split('\n').map((t) => t.replace(/^v/, '')))
  tagged.delete(VERSION)
  if (!tagged.size) return SKIP('older versions named in prose', 'no other release tags in this checkout')

  // GROUPED BY THE SENTENCE, not listed per file. "1.0.0 closes the beta period" is one editorial
  // decision that happens to be pasted into eight pages (three of them generated FROM the other
  // five), and printing it eight times reads as eight problems and gets skimmed.
  const byClaim = new Map()
  for (const abs of docFiles()) {
    readFileSync(abs, 'utf8').split('\n').forEach((line, i) => {
      // A dotted quad is an address, not a version — the lookarounds are what keep 127.0.0.1 and
      // "ADIF 3.1.4" out. Only a string this repo has actually TAGGED counts as a version.
      const named = [...new Set([...line.matchAll(/(?<![\w.])(\d+\.\d+\.\d+)(?![\w.])/g)].map((m) => m[1]))]
        .filter((v) => tagged.has(v))
      if (!named.length) return
      const claim = line.trim().replace(/^[>*\-\s]+/, '').slice(0, 110)
      const key = `${named.sort().join('/')}|${claim}`
      if (!byClaim.has(key)) byClaim.set(key, { named, claim, where: [] })
      byClaim.get(key).where.push(`${rel(abs)}:${i + 1}`)
    })
  }
  if (byClaim.size) {
    REVIEW('older versions named in prose',
      [...byClaim.values()]
        .sort((a, b) => b.where.length - a.where.length)
        .map((c) => `${c.where.length}x [${c.named.join(', ')}] ${c.claim}\n             ${c.where.join(' · ')}`),
      'Some are correct history ("1.0.0 installs over 0.27.0", "if you ran 0.24.0 through ' +
        '0.27.x") and some are a banner that stopped being news three releases ago. Read each ' +
        `one against ${VERSION} and decide; nothing here is automatically wrong.`)
  } else {
    OK('older versions named in prose', 'no doc names a superseded release')
  }
}

// ---------------------------------------------------------------------------
// The steps no script can do
// ---------------------------------------------------------------------------

const MANUAL = [
  ['SourceForge wiki — a HAND PASTE, every release',
   'docs/launch/wiki/*.md is the SOURCE and it does NOT sync anywhere. The SF wiki is a ' +
   'CodeMirror editor: set document.querySelector(".CodeMirror").CodeMirror.setValue(<text>) ' +
   'then Save — a plain textarea.value is clobbered on submit. The Allura bearer token does ' +
   'REST; the SSH key does git and FRS. Do not confuse them.'],
  ['GitHub wiki — the same paste, the same source',
   'The three generated pages plus the four hand-written ones (Home, Rig-Setup, ' +
   '_ProjectSummary, Documentation) live only in docs/launch/wiki/ until someone pastes them.'],
  ['The site copy of the manual',
   'CI cannot do this — it needs the site checkout, which exists only on the maintainer machine. ' +
   'Copy what changed, rebuild, deploy, then CURL THE LIVE URL: wrangler pages deploy can exit 0 ' +
   'having deployed nothing.'],
  ['SourceForge Default Download',
   'Browser only (a scripted POST returns 401). Files -> the new .exe -> (i) -> Default ' +
   'Download For: Windows -> Save, and check LINUX too — they flip more than one platform. ' +
   'best_release.json is the authoritative confirmation; the Files page banner caches and lies.'],
]

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

console.log(`release-docs — version ${VERSION}`)
console.log(`repo: ${ROOT}\n`)

checkGenerators()
checkDocGates()
checkScreenshots()
checkReleaseDocs()
checkIssueCredits()
checkVersionProse()

const ICON = { ok: '  ok  ', fail: ' FAIL ', review: 'REVIEW', skip: ' skip ' }
const show = (r) => {
  console.log(`[${ICON[r.severity]}] ${r.name}${r.note && r.severity === 'ok' ? ` — ${r.note}` : ''}`)
  for (const l of r.lines) console.log(`           ${l}`)
  if (r.note && r.severity !== 'ok') console.log(`           → ${r.note}`)
}

console.log('── checks ' + '─'.repeat(60))
results.filter((r) => r.severity === 'ok' || r.severity === 'skip').forEach(show)

const fails = results.filter((r) => r.severity === 'fail')
const reviews = results.filter((r) => r.severity === 'review')

if (fails.length) {
  console.log('\n── FAIL ' + '─'.repeat(62))
  fails.forEach(show)
}
if (reviews.length) {
  console.log('\n── REVIEW (a human decides; these do not fail the run) ' + '─'.repeat(16))
  reviews.forEach(show)
}

console.log('\n── BY HAND — nothing above can check these ' + '─'.repeat(28))
for (const [title, body] of MANUAL) console.log(`  * ${title}\n      ${body.replace(/(.{92}) /g, '$1\n      ')}`)

console.log(
  `\n${fails.length} fail · ${reviews.length} review · ` +
    `${results.filter((r) => r.severity === 'ok').length} ok · ` +
    `${results.filter((r) => r.severity === 'skip').length} skipped`,
)
process.exit(fails.length ? 1 : 0)
