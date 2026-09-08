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
//        node scripts/release-docs.mjs --push-gate       (the per-commit subset CI runs)
//
// The order matters and is not arbitrary — see .claude/skills/release-docs/SKILL.md.

import { readFileSync, readdirSync, existsSync, statSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
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

// docs-coverage is the one that asks whether a shipped capability is documented AT ALL —
// the other two only check that documentation which exists is correct, and were green
// through four releases in which PSK had no chapter.
const DOC_TESTS = [
  'src/docs-match-code.test.ts',
  'src/docs-settings-pointers.test.ts',
  'src/docs-coverage.test.ts',
]

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
//
// WHY THIS IS A PREFIX TABLE AND NOT ONE LINE PER IMAGE. It was one line per image while there
// were eleven. The illustrated manual is going to ~90, and 33 of those are Settings tabs that
// all show the same two files — so a per-image table would be 33 identical right-hand sides
// maintained by hand, on eight branches at once, all editing the same object literal. That is
// not a gate anyone keeps; it is a merge conflict with a check attached.
//
// So a key is a FAMILY PREFIX, and the LONGEST key that a filename starts with wins. Order in
// this object is irrelevant — you cannot break another family's mapping by where you paste
// yours, which is the footgun a first-match-wins list would have.
//
//   'settings-'        matches settings-radio.webp, settings-audio.webp, all 33 of them
//   'operate-'         matches operate-waterfall.webp
//   'operate-classic'  is longer, so it beats 'operate-' for operate-classic.webp
//
// HOW TO ADD AN IMAGE. Name it for what it shows (see scripts/build-manual-images.py). If its
// family prefix is already here, you add NOTHING — the new capture is checked the moment it
// lands. If it is a new family, add ONE key naming the source files whose change would make
// the picture wrong. Do not add a key that names a whole directory or a barrel file: an owner
// that changes every week reports staleness every week, and that is the same as reporting none.
//
// A key that matches no image today is NOT a failure — it is a family nobody has captured yet,
// costs nothing, and pre-declaring it is what keeps eight branches out of each other's diffs.
// (An exact per-image key that matched nothing WAS a failure, because it meant a real image had
// been renamed out from under its mapping. A prefix cannot hide that: an image matching no key
// still FAILS.) Unused keys are counted in the ok line so a dead family stays visible.

const SHOT_OWNERS = {
  // Setup — the first-run wizard.
  'wizard-': ['ui/src/components/SetupWizard.tsx'],
  'wizard-rig': ['ui/src/components/SetupWizard.tsx', 'ui/src/components/SetupHealth.tsx'],

  // Settings. One family, 33 captures: the panel and the registry that generates it.
  'settings-': ['ui/src/components/SettingsPanel.tsx', 'ui/src/settings/registry.ts'],

  // The FT8/FT4 cockpit and its panes.
  'operate-': ['ui/src/components/OperateCockpit.tsx'],
  'operate-classic': ['ui/src/components/OperateCockpit.tsx', 'ui/src/components/OperateDecodes.tsx'],
  'operate-roster': ['ui/src/components/OperateCockpit.tsx', 'ui/src/components/OperateRoster.tsx'],
  'operate-waterfall': ['ui/src/components/Waterfall.tsx'],

  // The other mode cockpits.
  'cw-': ['ui/src/components/CwCockpit.tsx'],
  'phone-': ['ui/src/components/PhoneCockpit.tsx'],
  'rtty-': ['ui/src/components/RttyCockpit.tsx'],
  'psk-': ['ui/src/components/PskCockpit.tsx'],
  'js8-': ['ui/src/components/Js8Cockpit.tsx'],
  'sstv': ['ui/src/components/SstvView.tsx'],
  'aprs-': ['ui/src/components/AprsCockpit.tsx'],
  'tempo-': ['ui/src/components/Conversation.tsx', 'ui/src/components/Composer.tsx'],

  // Hunting, spotting, events.
  'needed-': ['ui/src/components/NeededPanel.tsx'],
  'spots-': ['ui/src/components/SpotsPanel.tsx'],
  'dxpeditions-': ['ui/src/components/DxpeditionsView.tsx'],
  'pota-': ['ui/src/components/PotaSotaView.tsx'],
  'contest-': ['ui/src/components/ContestCalendarPane.tsx'],

  // Log, awards, statistics.
  'logbook-': ['ui/src/components/Logbook.tsx'],
  'logbook-entry': ['ui/src/components/Logbook.tsx', 'ui/src/components/LogEntry.tsx'],
  'awards-official': ['ui/src/components/AwardsView.tsx'],
  'awards-journey': ['ui/src/components/AwardsJourney.tsx'],
  'stats-': ['ui/src/components/StatsView.tsx'],

  // Maps, memories, programming, satellites.
  'connect-': ['ui/src/components/ConnectView.tsx'],
  'connect-map': ['ui/src/components/ConnectView.tsx', 'ui/src/components/MapView.tsx'],
  'memories-': ['ui/src/components/MemoriesView.tsx'],
  'memory-': ['ui/src/components/MemoriesView.tsx'],
  'program-': ['ui/src/components/RadioProgView.tsx'],
  'satellites-': ['ui/src/components/SatellitesView.tsx'],
}

/** The longest family prefix this filename starts with, or null when nothing claims it. */
function ownerKeyFor(name) {
  let best = null
  for (const key of Object.keys(SHOT_OWNERS)) {
    if (name.startsWith(key) && (best === null || key.length > best.length)) best = key
  }
  return best
}

function checkScreenshotMapping() {
  const dir = path.join(DOCS, 'img', 'manual')
  if (!existsSync(dir)) {
    SKIP('manual screenshots', `${rel(dir)} does not exist`)
    return null
  }
  const entries = readdirSync(dir).filter((f) => !statSync(path.join(dir, f)).isDirectory())

  // build-manual-images.py emits WEBP and only WEBP, so anything else here is a raw capture
  // somebody committed by mistake — a 3251-px JPEG that will ship at ten times the byte cost
  // and render unreadable. It is caught here rather than in review because 75 raw captures are
  // sitting in a handoff bundle one `cp` away from this directory.
  const notWebp = entries.filter((f) => !f.endsWith('.webp'))
  const images = entries.filter((f) => f.endsWith('.webp'))

  // Positive control on the mapping itself, in both directions.
  const SHOTS = Object.fromEntries(
    images.map((f) => [f, SHOT_OWNERS[ownerKeyFor(f)]]).filter(([, owners]) => owners),
  )
  const unmapped = images.filter((f) => ownerKeyFor(f) === null)
  const usedKeys = new Set(images.map(ownerKeyFor).filter(Boolean))
  const unusedKeys = Object.keys(SHOT_OWNERS).filter((k) => !usedKeys.has(k))
  const badOwners = Object.entries(SHOT_OWNERS).flatMap(([key, owners]) =>
    owners.filter((o) => !existsSync(path.join(ROOT, o))).map((o) => `${key}* -> ${o} does not exist`))
  const mappingProblems = [
    ...unmapped.map((f) =>
      `docs/img/manual/${f} matches no SHOT_OWNERS family — it is being checked by nobody`),
    ...notWebp.map((f) =>
      `docs/img/manual/${f} is not a .webp — run scripts/build-manual-images.py, do not commit the raw capture`),
    ...badOwners,
  ]
  if (mappingProblems.length) {
    FAIL('screenshot mapping', mappingProblems,
      'Fix SHOT_OWNERS in scripts/release-docs.mjs — one key per FAMILY, not per image. Read the ' +
        'block comment above it before adding a key.')
  } else {
    OK('screenshot mapping',
      `${images.length} images mapped by ${usedKeys.size} of ${Object.keys(SHOT_OWNERS).length} ` +
        `families, all naming source that exists` +
        (unusedKeys.length ? ` · ${unusedKeys.length} families not captured yet: ${unusedKeys.join(', ')}` : ''))
  }
  return { SHOTS, mappingProblems }
}

// Split from the mapping deliberately: the mapping is true of every commit and is offline, so it
// is a push gate. THIS half reads git history, and CI checks out at depth 1 — on a shallow clone
// `git log` answers nothing and every image would look current, which is a false green rather
// than a missing check. So it runs at release time, where the history is there.
function checkScreenshotFreshness(SHOTS, mappingProblems) {
  // RANKED, not just listed. A commit that touches every cockpit at once (1db7d051 did) puts
  // eight images on this list at equal weight, and a flat list of eight is a list nobody reads.
  // The commit COUNT since the capture is the closest honest proxy for how much of the frame has
  // moved, so it goes first and the list is sorted by it.
  const stale = []
  const pending = []
  for (const [img, owners] of Object.entries(SHOTS)) {
    const imgPath = `docs/img/manual/${img}`
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
// 3b. The assets the pages point at — existence, alt text, orphans, duplicates
// ---------------------------------------------------------------------------
//
// Section 3 asks whether an image is STALE. This asks whether it is THERE, and whether the page
// around it is intact. Nothing did, and every one of these has a way of reaching the public in
// silence:
//
//   * a page points at an image that is not in the tree — the reader gets a broken-image icon,
//     and neither the EPUB nor the PDF build says a word about it;
//   * an image is in the tree that no page points at — which is not a wasted 200 KB, it is a
//     capture somebody cropped and published and then never actually placed;
//   * an image ships with empty alt text — invisible to a sighted reviewer by construction,
//     and the only thing a screen-reader user gets;
//   * the same bytes ship twice under two names — one capture, presented as two things.
//
// This is I29's mechanical half. The judgement half (is the picture the RIGHT picture) is not
// automatable and is not attempted here.

// A local target that this repo can actually answer for. Anything remote, inline or
// site-absolute is somebody else's to verify.
const isLocalRef = (t) => !/^(?:[a-z][a-z0-9+.-]*:|\/\/|\/|#)/i.test(t)

/** Every `![alt](src)` and `<img src alt>` in `text`, with 1-based line numbers. */
function imageRefs(text) {
  const out = []
  text.split('\n').forEach((line, i) => {
    for (const m of line.matchAll(/!\[([^\]]*)\]\(\s*([^)\s]+)(?:\s+"[^"]*")?\s*\)/g))
      out.push({ line: i + 1, alt: m[1], src: m[2] })
    for (const m of line.matchAll(/<img\b[^>]*>/g)) {
      const src = /\bsrc="([^"]*)"/.exec(m[0])
      // A missing alt attribute and alt="" are the same defect to a screen reader, and both
      // arrive here as ''.
      out.push({ line: i + 1, alt: (/\balt="([^"]*)"/.exec(m[0]) ?? ['', ''])[1], src: src ? src[1] : '' })
    }
  })
  return out
}

// An alt that says only "image" describes nothing; it is the shape of a placeholder somebody
// meant to come back to. Kept short and literal — a length threshold would be arbitrary, and
// "Settings ▸ Radio" is a perfectly good short alt.
const PLACEHOLDER_ALT = new Set([
  'image', 'images', 'img', 'screenshot', 'screen shot', 'screengrab', 'picture', 'photo',
  'figure', 'fig', 'diagram', 'graphic', 'alt', 'alt text', 'todo', 'tbd',
])

function checkDocAssets() {
  const manualDir = path.join(DOCS, 'img', 'manual')
  const problems = []
  const referenced = new Set()

  for (const abs of docFiles()) {
    for (const { line, alt, src } of imageRefs(readFileSync(abs, 'utf8'))) {
      if (!isLocalRef(src)) continue
      const target = path.resolve(path.dirname(abs), src)
      referenced.add(target)
      if (!existsSync(target)) {
        problems.push(`${rel(abs)}:${line} points at ${src} — no such file`)
        continue
      }
      const trimmed = alt.trim()
      if (!trimmed) problems.push(`${rel(abs)}:${line} ${path.basename(src)} has empty alt text`)
      else if (PLACEHOLDER_ALT.has(trimmed.toLowerCase()))
        problems.push(`${rel(abs)}:${line} ${path.basename(src)} alt is the placeholder "${trimmed}"`)
    }
  }

  // Orphans and duplicates are asked of docs/img/manual/ ONLY. The rest of docs/img/ is the
  // site's and SourceForge's — banners, the social card, the demo GIF — referenced from repos
  // that are not this one, so "unreferenced" there means nothing. This directory exists to
  // serve manual pages and nothing else, so there it means everything.
  if (existsSync(manualDir)) {
    const files = readdirSync(manualDir)
      .map((f) => path.join(manualDir, f))
      .filter((p) => !statSync(p).isDirectory())
    for (const p of files)
      if (!referenced.has(p)) problems.push(`${rel(p)} is published but no page shows it`)

    const byHash = new Map()
    for (const p of files) {
      const h = createHash('sha256').update(readFileSync(p)).digest('hex')
      if (!byHash.has(h)) byHash.set(h, [])
      byHash.get(h).push(rel(p))
    }
    for (const names of byHash.values())
      if (names.length > 1) problems.push(`identical bytes under ${names.length} names: ${names.join(', ')}`)
  }

  if (problems.length) {
    FAIL('manual assets', problems,
      'Each is mechanical: place the image or drop the reference, write the alt text, delete the ' +
        'duplicate. Alt text describes the PICTURE for somebody who cannot see it — it is not a ' +
        'caption and not a repeat of the sentence above it.')
  } else {
    OK('manual assets', `${referenced.size} referenced images all present, alt-texted and placed once`)
  }
}

// ---------------------------------------------------------------------------
// 3c. Internal links and anchors
// ---------------------------------------------------------------------------
//
// The other half of I29. A cross-reference between chapters is the manual's only navigation, and
// a heading gets reworded in every editing pass — at which point every deep link into it points
// at the top of the page instead, silently, in the repo and on the site and in the EPUB.
//
// Only LOCAL links are checked. An external URL needs the network and would make this check
// flap; the site's own copy is checked separately, against the live index.

/** GitHub's heading slug: strip inline markup, lowercase, drop punctuation, spaces to hyphens. */
function slugify(heading) {
  return heading
    .replace(/`([^`]*)`/g, '$1')
    .replace(/\*\*?([^*]*)\*\*?/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .trim()
    .toLowerCase()
    .replace(/[^\w\- ]/gu, '')
    .replace(/ /g, '-')
}

/** Non-fenced lines of a markdown file, as `[lineNumber, text]`. Fenced code is not prose. */
function proseLines(text) {
  const out = []
  let fence = null
  text.split('\n').forEach((line, i) => {
    const m = /^\s*(```+|~~~+)/.exec(line)
    if (m) {
      if (fence === null) fence = m[1][0]
      else if (m[1][0] === fence) fence = null
      return
    }
    if (fence === null) out.push([i + 1, line.replace(/`[^`]*`/g, '')])
  })
  return out
}

/** Every heading anchor in the file, including GitHub's `-1`, `-2` suffixes for repeats. */
function anchorsOf(abs) {
  const seen = new Map()
  const anchors = new Set()
  for (const [, line] of proseLines(readFileSync(abs, 'utf8'))) {
    const m = /^#{1,6}\s+(.*)$/.exec(line)
    if (!m) continue
    const base = slugify(m[1])
    const n = seen.get(base) ?? 0
    seen.set(base, n + 1)
    anchors.add(n === 0 ? base : `${base}-${n}`)
  }
  return anchors
}

function checkDocLinks() {
  const anchorCache = new Map()
  const anchors = (abs) => {
    if (!anchorCache.has(abs)) anchorCache.set(abs, anchorsOf(abs))
    return anchorCache.get(abs)
  }

  const broken = []
  let checked = 0
  for (const abs of docFiles()) {
    for (const [line, text] of proseLines(readFileSync(abs, 'utf8'))) {
      for (const m of text.matchAll(/(?<!!)\[[^\]]*\]\(\s*([^)\s]+)(?:\s+"[^"]*")?\s*\)/g)) {
        const target = m[1]
        const [file, anchor] = target.split('#')
        if (file && !isLocalRef(file)) continue
        checked++
        let dest = abs
        if (file) {
          // The wiki pages link each other extensionless (`[Install](Install)`), which is how
          // they resolve on the GitHub and SourceForge wikis. In the repo that is Install.md.
          const candidates = [path.resolve(path.dirname(abs), file), path.resolve(path.dirname(abs), `${file}.md`)]
          dest = candidates.find((c) => existsSync(c))
          if (!dest) {
            broken.push(`${rel(abs)}:${line} → ${target} — no such file`)
            continue
          }
        }
        if (anchor && dest.endsWith('.md') && !anchors(dest).has(anchor))
          broken.push(`${rel(abs)}:${line} → ${target} — ${rel(dest)} has no heading with that anchor`)
      }
    }
  }

  if (broken.length) {
    FAIL('internal links and anchors', broken,
      'A renamed heading breaks every link into it and nothing says so. Fix the link, or restore ' +
        'the heading — do not delete the cross-reference to make this green.')
  } else {
    OK('internal links and anchors', `${checked} local links resolve, anchors included`)
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
//
// TWO THINGS IT USED TO GET WRONG, both found on 1.10.3 and both the sort that gets a check
// switched off rather than obeyed:
//
//   * `gh issue list` cannot see DISCUSSIONS. 1.10.3 credits "(#231, #232)" — #231 is the issue
//     and #232 is the discussion the same fault was reported in, which is a correct credit and
//     was reported as "#232 is not an issue in kd9taw/Nexus". A number is now looked up as a
//     discussion before it is called missing.
//   * "cited ⇒ must be CLOSED" is not true of a fix the maintainer cannot yet prove. 1.10.3
//     shipped two fixes marked **NEEDS-BENCH** — the project-wide flag for "the change is in,
//     the hardware to confirm it on is not here" (#126 wants an FTDX-101D, #233 a Mac). Those
//     issues stay open on purpose, possibly for months, and there is no repo edit that clears
//     them: a permanent FAIL nobody can act on is exactly how the FAILs stop being read. So an
//     open issue whose own CHANGELOG bullet carries NEEDS-BENCH is REVIEW, listed by name, and
//     an open issue with no such marker is still a FAIL. NEEDS-BENCH is not a mute button: it
//     is published to the operator in the release notes, so writing a false one is a public
//     claim, not a private silencing.

function versionSection(changelog, version) {
  const start = changelog.search(new RegExp(`^## \\[${version.replace(/\./g, '\\.')}\\]`, 'm'))
  if (start < 0) return null
  const after = changelog.slice(start + 1)
  const end = after.search(/^## \[/m)
  return end < 0 ? after : after.slice(0, end)
}

/** The section's top-level bullets, each running to the next one, so a citation keeps its prose. */
function bullets(section) {
  const out = []
  for (const line of section.split('\n')) {
    if (/^- /.test(line) || !out.length) out.push(line)
    else out[out.length - 1] += `\n${line}`
  }
  return out
}

/**
 * `{ title, closed }` when `n` is a discussion, `null` when GitHub says it is definitively not
 * one, `undefined` when the question could not be asked at all.
 */
function discussion(n) {
  const query = `{repository(owner:"kd9taw",name:"Nexus"){discussion(number:${n}){title closed}}}`
  try {
    const out = execFileSync('gh', ['api', 'graphql', '-f', `query=${query}`], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    return JSON.parse(out)?.data?.repository?.discussion ?? null
  } catch (e) {
    // gh exits non-zero for a number that is not a discussion AND for a network failure. Only
    // the first is an answer; the second must not be reported as one.
    return /NOT_FOUND|Could not resolve to a Discussion/.test(`${e.stdout ?? ''}${e.stderr ?? ''}`)
      ? null
      : undefined
  }
}

function checkIssueCredits() {
  const section = versionSection(readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8'), VERSION)
  if (section == null) return SKIP('CHANGELOG issue credits', `no [${VERSION}] section to read`)
  const cited = [...new Set([...section.matchAll(/#(\d{1,5})\b/g)].map((m) => m[1]))]
  if (!cited.length) return OK('CHANGELOG issue credits', `the [${VERSION}] section cites no issues`)

  // Which citations sit in a bullet that already says the fix is unproven.
  const needsBench = new Set()
  for (const b of bullets(section))
    if (/NEEDS-BENCH/.test(b)) for (const m of b.matchAll(/#(\d{1,5})\b/g)) needsBench.add(m[1])

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

  const open = []
  const unproven = []
  const missing = []
  const unknown = []
  const discussions = []
  for (const n of cited) {
    const issue = byNumber.get(n)
    if (!issue) {
      const d = discussion(n)
      if (d) discussions.push(`#${n} "${d.title}" (discussion${d.closed ? ', closed' : ''})`)
      else if (d === null) missing.push(`#${n} is neither an issue nor a discussion in kd9taw/Nexus`)
      else unknown.push(`#${n} is not in the issue list and the discussion lookup could not run — check by hand`)
      continue
    }
    if (issue.state === 'CLOSED') continue
    const line = `#${issue.number} "${issue.title}" is ${issue.state} — the CHANGELOG credits it as done in ${VERSION}`
    if (needsBench.has(n)) unproven.push(`${line}, and its bullet says NEEDS-BENCH`)
    else open.push(line)
  }

  if (open.length || missing.length) {
    FAIL('CHANGELOG issue credits', [...open, ...missing],
      'A released section is immutable history, so do not quietly rewrite it: correct it in the ' +
        'NEXT version section and, if the issue is genuinely not fixed, say so on the issue. ' +
        'Never close an issue because the CHANGELOG claimed it.')
  } else {
    OK('CHANGELOG issue credits',
      `#${cited.join(', #')} — every credit resolves and none is an open claim of a fix` +
        (discussions.length ? ` · credits a discussion: ${discussions.join(', ')}` : ''))
  }
  if (unproven.length) {
    REVIEW('issues credited but still open on purpose', unproven,
      'Each is a fix that shipped and cannot be proved without hardware the maintainer does not ' +
        'have. Nothing in the repo closes these — they close when somebody puts the change on a ' +
        'real radio. Check the flag still belongs before you skip past it.')
  }
  if (unknown.length) {
    REVIEW('issue credits that could not be resolved', unknown,
      'The lookup failed, which is not evidence the credit is fine.')
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
// Is every manual chapter actually ON the live site?
// ---------------------------------------------------------------------------
//
// The site's copy of the manual is synced by hand (`scripts/sync-manual.mjs` needs a Nexus
// checkout, which no runner has), so it only moves when somebody remembers. It had drifted for
// FOUR releases before anyone noticed, and what finally noticed was 1.7.6's release notes
// advertising a Tempo chapter that 404'd on the live site — i.e. an operator following the
// release notes would have found the promise empty.
//
// This used to be a line in the BY HAND list below. A note is what let it rot: it says "go and
// check" and nothing happens if you don't. So it is a CHECK now, and it asks the live site
// rather than the repo, because the repo has been right the whole time — the site was wrong.
//
// It is skipped under --push-gate: the answer is about a deploy, not about a commit, and a
// network call inside a per-push CI step is a gate that flaps.
if (!flag('--push-gate')) try {
  const chapters = readdirSync(path.join(ROOT, 'docs', 'guide'))
    .filter((f) => f.endsWith('.md') && f !== 'index.md')
    .map((f) => f.replace(/\.md$/, ''))
  const res = await fetch('https://hamradiotools.io/manual/', { redirect: 'follow' })
  const html = res.ok ? await res.text() : ''
  const live = new Set([...html.matchAll(/href="\/manual\/([a-z0-9-]+)"?/g)].map((m) => m[1]))
  if (!res.ok || live.size === 0) {
    // A "nothing found" here would otherwise read as "every chapter is missing" and cry wolf.
    REVIEW('site manual vs docs/guide', [`could not read the live manual index (HTTP ${res.status})`],
      'Not a verdict about the manual — the CHECK could not run. Try again, or look by hand.')
  } else {
    const missing = chapters.filter((c) => !live.has(c))
    if (missing.length) {
      FAIL('site manual vs docs/guide', missing.map((c) => `${c} — docs/guide/${c}.md is not on hamradiotools.io/manual/`),
        'Run scripts/sync-manual.mjs in the site checkout, commit, push it to the site repo main, ' +
        'then re-deploy (gh workflow run publish.yml -f tag=v<version>) and re-run this.')
    } else {
      OK('site manual vs docs/guide', `all ${chapters.length} chapters are live (${live.size} pages on the index)`)
    }
  }
} catch (e) {
  REVIEW('site manual vs docs/guide', [String(e && e.message ? e.message : e)],
    'The check itself failed (offline?). That is not evidence the manual is fine.')
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

// --push-gate is the subset that is true of EVERY COMMIT rather than only of a release: no
// network, no `gh`, no git history, and nothing that asks a question only a release can answer
// (is there a RELEASE_NOTES for this version, is the CHANGELOG stamped, are its issue credits
// closed — all of which are legitimately unfinished on a topic branch and would be red on every
// push). ci.yml runs this in the `ui` job beside the other doc gates, because a check whose only
// trigger is somebody remembering to run release-docs is not a gate — which is the argument this
// whole file was written to make, and it applied to the file itself.
const PUSH_GATE = flag('--push-gate')

console.log(PUSH_GATE ? 'release-docs --push-gate' : `release-docs — version ${VERSION}`)
console.log(`repo: ${ROOT}\n`)

if (!PUSH_GATE) {
  checkGenerators()
  checkDocGates()
}
const shots = checkScreenshotMapping()
checkDocAssets()
checkDocLinks()
if (!PUSH_GATE) {
  if (shots) checkScreenshotFreshness(shots.SHOTS, shots.mappingProblems)
  checkReleaseDocs()
  checkIssueCredits()
  checkVersionProse()
}

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

if (!PUSH_GATE) {
  console.log('\n── BY HAND — nothing above can check these ' + '─'.repeat(28))
  for (const [title, body] of MANUAL) console.log(`  * ${title}\n      ${body.replace(/(.{92}) /g, '$1\n      ')}`)
}

console.log(
  `\n${fails.length} fail · ${reviews.length} review · ` +
    `${results.filter((r) => r.severity === 'ok').length} ok · ` +
    `${results.filter((r) => r.severity === 'skip').length} skipped`,
)
process.exit(fails.length ? 1 : 0)
