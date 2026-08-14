import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import {
  SETTINGS_TABS,
  SETTINGS_SECTIONS,
  TAB_ALIASES,
  SECTION_ALIASES,
  resolveTarget,
  sectionSlug,
} from './settings/registry'

// DOC PROSE MUST NOT NAME A SETTINGS TAB OR SECTION THAT NO LONGER EXISTS.
//
// The 1.3.0 Settings reorganisation (eight tabs → nine; "Modes" split into Phone/CW/Digital;
// "Rig Control" → "Rig & CAT"; Frequencies folded into Digital) outdated roughly thirty-four
// doc files, and every one of them was found BY HAND, after the release had already shipped.
// Nothing went red. `docs-match-code.test.ts` guards the settings reference page's `##`
// headings and every `](…settings-reference.md#anchor)` link, so a dead ANCHOR fails — but a
// sentence in docs/manual/Getting-Started.md reading "Settings ▸ Modes ▸ CW" is ordinary prose
// with no link in it, and no guard in this repo could see it.
//
// THE THING THAT MAKES THIS CHECKABLE RATHER THAN A JUDGEMENT CALL is that the registry already
// has to declare its own renames: `TAB_ALIASES` and `SECTION_ALIASES` exist so a stale deep link
// still lands somewhere sensible (registry.ts:401-449). That makes them a DECLARED LIST OF DEAD
// NAMES, maintained for a reason that has nothing to do with docs. So this guard flags a name
// only when the registry itself says it is legacy.
//
// ⚠️ THAT CONSERVATISM IS THE WHOLE DESIGN, AND IT IS WHY THIS GATE IS NOT NOISE. The doc set
// documents OTHER PROGRAMS' settings menus constantly — "Settings ▸ Time & Language" (Windows),
// "Settings › Application Program Interface" (N3FJP), "Settings ▸ Apps", "Settings ▸ Camera".
// A guard that flagged every unrecognised name after the word "Settings" would fire on all of
// them, and would be switched off within a release. A name the registry has never heard of is
// therefore IGNORED, deliberately: the cost is a missed typo, and the benefit is a gate that
// survives.
//
// When this goes red the DOC is what changes, not the registry — unless the rename itself was
// wrong. The failure message names where the content lives now, resolved through the registry's
// own `resolveTarget()`, so the fix is a rewrite and not an investigation.
//
// What this CANNOT see is written down at the END OF THIS FILE. Read it before trusting a green
// run.

const repo = (rel: string) => fileURLToPath(new URL(`../../${rel}`, import.meta.url))
const read = (abs: string) => readFileSync(abs, 'utf8')
const rel = (abs: string) => abs.slice(repo('').length).replace(/\\/g, '/')

/** Every .md under docs/, plus the README — the surfaces a user reads. */
function docCorpus(): string[] {
  const out: string[] = []
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const abs = join(dir, name)
      if (statSync(abs).isDirectory()) walk(abs)
      else if (name.endsWith('.md')) out.push(abs)
    }
  }
  walk(repo('docs'))
  out.push(repo('README.md'))
  return out.sort()
}

/**
 * Release notes are a POINT-IN-TIME RECORD. "The Modes tab now carries…" was true in 1.0.2 and
 * rewriting it would be the dishonest fix. Same rule `docs-match-code.test.ts:385` applies to its
 * mode guard, and for the same reason.
 */
const isHistory = (abs: string) => /RELEASE_NOTES-/.test(abs)

/** Lines outside fenced code blocks, so a sample config can't read as prose. */
function proseLines(md: string): (string | null)[] {
  let fenced = false
  return md.split('\n').map((l) => {
    if (/^\s*(```|~~~)/.test(l)) {
      fenced = !fenced
      return null
    }
    return fenced ? null : l
  })
}

// ---------------------------------------------------------------------------
// The two name sets, both read out of the registry
// ---------------------------------------------------------------------------

const norm = (s: string) => s.trim().toLowerCase().replace(/\s+/g, ' ')

/**
 * Every spelling of a live name, split by DEPTH — which matters, because the registry models a
 * pointer only as far as its section. `Settings ▸ Radio ▸ Rig & CAT ▸ CAT Broker` is a correct,
 * current pointer whose last segment is a CONTROL, and controls are not registry data. Reading
 * that segment as a tab name flags "CAT" (a legacy tab) and calls a right sentence wrong. So the
 * walk stops at the first live SECTION and keeps going through a live TAB.
 */
const TAB_NAMES = new Set<string>()
for (const t of SETTINGS_TABS) {
  TAB_NAMES.add(norm(t.label))
  TAB_NAMES.add(norm(t.id))
  TAB_NAMES.add(sectionSlug(t.label))
}
const SECTION_NAMES = new Set<string>()
for (const s of SETTINGS_SECTIONS) {
  SECTION_NAMES.add(norm(s.label))
  SECTION_NAMES.add(norm(s.id))
  SECTION_NAMES.add(sectionSlug(s.label))
}
const LIVE = new Set<string>([...TAB_NAMES, ...SECTION_NAMES])

/**
 * Names the registry declares dead, minus anything still live under another meaning.
 *
 * The subtraction is load-bearing. `TAB_ALIASES` carries `alerts`, `connections`, `features`,
 * `workspace` and `audio` — each of those was a TAB once and is a live SECTION now, so the
 * alias exists to redirect a bare tab reference while the word itself is still correct on the
 * page. Flagging them would make the guard wrong about five names it should stay quiet on.
 */
const LEGACY = new Map<string, string>() // dead name -> the raw string to hand resolveTarget()
for (const name of Object.keys(TAB_ALIASES)) {
  if (!LIVE.has(norm(name))) LEGACY.set(norm(name), name)
}
for (const slug of Object.keys(SECTION_ALIASES)) {
  if (!LIVE.has(norm(slug))) LEGACY.set(slug, slug)
}

/** Where the registry says a dead name's content lives now, phrased for the person fixing it. */
function landsAt(raw: string): string {
  const t = resolveTarget(raw)
  if (!t) return 'nowhere the registry can name — decide where it belongs and rewrite the sentence'
  const tab = SETTINGS_TABS.find((x) => x.id === t.tab)
  const sec = t.section ? SETTINGS_SECTIONS.find((x) => x.id === t.section) : undefined
  return sec ? `Settings ▸ ${tab?.label} ▸ ${sec.label}` : `Settings ▸ ${tab?.label}`
}

// ---------------------------------------------------------------------------
// Grammar 1 — a path pointer: "Settings ▸ Modes ▸ CW"
// ---------------------------------------------------------------------------

type Kind = 'tab' | 'section' | 'legacy'

/**
 * The longest known name at the START of `tail`, and what depth it sits at.
 *
 * Candidates are built forward, one to five words, rather than by trying to delimit the segment
 * first. A segment has no reliable end: the doc set writes "Settings ▸ Station (default …",
 * "Settings ▸ Rig now takes a separate …" and "Settings ▸ Digital." — three different
 * terminators, and the middle one has none at all. Matching a known name against the head of the
 * text sidesteps the question entirely.
 *
 * LONGEST WINS, and at equal length tab beats section beats legacy: "Logging & Connectors" must
 * not be read as the tab id `logging` plus stray words, "Rig & CAT" (live) must not be read as
 * "Rig" (legacy), and a name that is both a tab and a section ("CW", "Phone") is taken as the
 * tab so the walk carries one level deeper instead of stopping early.
 */
function nameAt(tail: string): { name: string; kind: Kind } | null {
  const words = tail.match(/^[\s]*((?:[A-Za-z0-9&/+.'’-]+[ \t]*){1,5})/)
  if (!words) return null
  const tokens = words[1].trim().split(/[ \t]+/)
  let best: { name: string; kind: Kind } | null = null
  for (let n = 1; n <= tokens.length; n++) {
    const cand = tokens.slice(0, n).join(' ')
    const forms = [norm(cand), sectionSlug(cand)]
    const kind: Kind | null = forms.some((f) => TAB_NAMES.has(f))
      ? 'tab'
      : forms.some((f) => SECTION_NAMES.has(f))
        ? 'section'
        : forms.some((f) => LEGACY.has(f))
          ? 'legacy'
          : null
    if (kind) best = { name: cand, kind }
  }
  return best
}

/**
 * Every `Settings <sep> …` pointer in the file, walked segment by segment until it bottoms out.
 *
 * ONE finding per pointer: the walk stops at the first dead name. A stale pointer is stale at its
 * root, and reporting each of its segments separately would make one wrong sentence look like
 * three.
 */
function pathPointers(md: string, file: string): string[] {
  const out: string[] = []
  proseLines(md).forEach((line, i) => {
    if (line == null) return
    for (const m of line.matchAll(/\bSettings\b/g)) {
      let at = m.index + 'Settings'.length
      // A pointer is only a pointer while separators keep coming, so "Settings is where…"
      // contributes nothing and costs one failed match.
      for (;;) {
        const rest = line.slice(at)
        const sep = /^[\s]*(▸|›|»|→|->|>)[\s]*/.exec(rest)
        if (!sep) break
        at += sep[0].length
        const hit = nameAt(line.slice(at))
        if (!hit) break
        if (hit.kind === 'legacy') {
          out.push(
            `${file}:${i + 1} points at "Settings ▸ ${hit.name}", which the registry declares ` +
              `DEAD (registry.ts TAB_ALIASES/SECTION_ALIASES). It now lives at ${landsAt(hit.name)}. ` +
              `Line: ${line.trim()}`,
          )
          break
        }
        // A live SECTION is the deepest thing the registry knows. Anything past it is a control
        // name, which is not registry data and must not be read as one.
        if (hit.kind === 'section') break
        at += line.slice(at).indexOf(hit.name) + hit.name.length
      }
    }
  })
  return out
}

// ---------------------------------------------------------------------------
// Grammar 2 — the bare phrase: "on the Modes tab"
// ---------------------------------------------------------------------------

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/**
 * `the <dead name> tab`. Built as an alternation over the LEGACY set only — never over
 * unrecognised words — so a sentence about WSJT-X's Reporting tab or N1MM's Configurer cannot
 * trip it. The `-` in the character class before `tab` lets "Rig-Control tab" match too.
 */
function tabPhrases(md: string, file: string): string[] {
  if (!LEGACY.size) return []
  const alt = [...LEGACY.values()].map(escape).join('|')
  const re = new RegExp(`\\b(?:the\\s+)?(${alt})\\s+tab\\b`, 'gi')
  const out: string[] = []
  proseLines(md).forEach((line, i) => {
    if (line == null) return
    for (const m of line.matchAll(re)) {
      const name = m[1]
      if (LIVE.has(norm(name))) continue
      out.push(
        `${file}:${i + 1} says "${m[0]}", but there is no ${name} tab — the registry declares ` +
          `that name dead. It now lives at ${landsAt(name)}. Line: ${line.trim()}`,
      )
    }
  })
  return out
}

// ---------------------------------------------------------------------------
// The guard
// ---------------------------------------------------------------------------

const CORPUS = docCorpus().filter((abs) => !isHistory(abs))

describe('doc prose names only Settings tabs and sections that exist', () => {
  // A guard that quietly matched nothing would go green forever. Three things are asserted
  // about the machinery itself before anything is asserted about the docs.
  it('has a corpus, a live name set and a declared dead-name set', () => {
    expect(CORPUS.length, 'no .md files found under docs/ — this guard is checking nothing').toBeGreaterThan(20)
    expect(LIVE.has('digital'), 'the live tab set did not come through').toBe(true)
    expect([...LEGACY.keys()], 'no dead names — TAB_ALIASES/SECTION_ALIASES stopped parsing').toContain('modes')
  })

  it('parses the pointers the doc set actually contains', () => {
    // The scanner must be shown to READ the real format, not merely to find nothing wrong with
    // it. If the separator set or the word class ever stops matching, this fails before the
    // silence below can be mistaken for cleanliness.
    const live = CORPUS.flatMap((abs) => {
      const md = read(abs)
      return proseLines(md).flatMap((l) => {
        if (l == null) return []
        return [...l.matchAll(/\bSettings\s*(?:▸|›|»|→|->|>)\s*/g)].flatMap((m) => {
          const hit = nameAt(l.slice(m.index + m[0].length))
          return hit && hit.kind !== 'legacy' ? [hit.name] : []
        })
      })
    })
    expect(new Set(live).size, 'no live "Settings ▸ <tab>" pointer parsed anywhere in docs/').toBeGreaterThan(4)
  })

  it('fires on a dead name (positive control)', () => {
    // Both grammars, against text this file owns. Without this, a regex typo makes every
    // assertion below pass by finding nothing — the failure mode CLAUDE.md calls out by name.
    const dead = 'docs/__control__.md'
    expect(pathPointers('Open Settings ▸ Modes ▸ CW and set the speed.\n', dead)).toHaveLength(1)
    expect(tabPhrases('Everything lives on the Modes tab now.\n', dead)).toHaveLength(1)
    // ...and stays quiet on a live name and on another program's menu.
    expect(pathPointers('Open Settings ▸ Digital ▸ RTTY.\n', dead)).toEqual([])
    expect(pathPointers('In Windows, open Settings ▸ Time & Language.\n', dead)).toEqual([])
    expect(tabPhrases('WSJT-X puts this on the Reporting tab.\n', dead)).toEqual([])
    // ...including on a CONTROL whose name starts with a dead tab name. This exact sentence is
    // in docs/manual/Rig-and-Audio-Setup.md, it is correct, and the first version of this guard
    // called it stale because "CAT" is a legacy tab and "CAT Broker" begins with it.
    expect(pathPointers('The CAT broker (Settings → Radio ▸ Rig & CAT → CAT Broker) is off.\n', dead)).toEqual([])
    // A dead name still fires when it is UNDER a live tab — a live tab does not end the walk.
    expect(pathPointers('Open Settings ▸ Digital ▸ Frequencies.\n', dead)).toHaveLength(1)
  })

  it('no doc points at a Settings tab or section that no longer exists', () => {
    const bad = CORPUS.flatMap((abs) => {
      const md = read(abs)
      return [...pathPointers(md, rel(abs)), ...tabPhrases(md, rel(abs))]
    })
    expect(bad).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// WHAT THIS GUARD DOES NOT CATCH — written down rather than chased with more guards.
//
// - A name the registry has NEVER declared. If a tab is renamed without adding the
//   `TAB_ALIASES` entry, every stale pointer to it reads as another program's menu and is
//   ignored. That entry is already required by registry.ts's own header, and `registry.test.ts`
//   does not enforce it either — so the discipline is human, and this guard inherits it.
// - A name that is dead as a TAB but live as a SECTION — "the Features tab", "the Alerts tab".
//   Five names sit in that state today and are deliberately excluded (see the LEGACY comment),
//   because flagging them would also flag the correct prose that names the section.
// - Anything outside the doc set. In-app strings, tooltips, `CHANGELOG.md`, the site, the
//   SourceForge wiki as PASTED (docs/launch/wiki/*.md is scanned, but the wiki itself is a
//   hand-paste that nothing in this repo can reach).
// - SCREENSHOTS. An image showing eight tabs is invisible here, and to every other guard in the
//   repo. `scripts/release-docs.mjs` is what checks those, against git history rather than
//   pixels.
// - The DESCRIPTION under a live name. "Settings ▸ Digital" may be correct while the sentence
//   after it describes a field that moved to Phone. Only the name is compared.
// - A pointer split across a line break. The doc set hard-wraps, and "Settings ▸\nModes" would
//   pass; scanning per line is what keeps the reported line number true.
//
// ⭐ THE BIGGEST GAP, AND IT IS THE DOMINANT FAILURE MODE OF A REORG — read this before
// trusting a green run. This guard only knows whether a NAME is alive. It cannot see a pointer
// naming a section that is alive but no longer HOLDS the control the sentence is about, because
// the registry stops at sections and controls are not registry data. A reorg that moves controls
// between LIVE sections is therefore completely invisible here, and 1.3.0 was exactly that reorg.
// Four such pointers were green under this guard and wrong on the page:
//   - the CAT broker, cited as `Rig & CAT ▸ CAT Broker` in three places. It moved to
//     `Transmit limits & sharing ▸ Share this radio with other programs`; only a residual
//     "Sharing port" field stays on Rig & CAT, and no control is called "CAT Broker" any more.
//   - N3FJP host/port and the N1MM address, aimed at `Contesting ▸ Field Day Setup`. That
//     section holds only the Field Day fields; both integrations live under
//     `Logging & Connectors ▸ N3FJP Integration` / `N1MM+ Integration`.
//
// ⚠️ DO NOT REDO THE OBVIOUS MECHANISATION — it was tried and MEASURED, and it does not work.
// The idea: take the segment after the last live section as a CONTROL name and require it to
// appear as text in SettingsPanel.tsx (comments stripped, so the code comment recording the move
// cannot vouch for the old name). Two measurements killed it:
//   1. THE POSITIVE CONTROL FAILED. Run against the pre-sweep doc corpus at 7e88d120 it flagged
//      ZERO and never reached "CAT Broker" — in that instance the PARENT was a dead name, so the
//      walk had already stopped, and the live-parent instances are precisely the ones with no
//      dead name to key on.
//   2. On the current corpus it flagged 10, ALL false, ALL the same shape: "Decode depth ▸ Fast"
//      is a control whose own value syntax contains the separator, so the segment slicer cuts it
//      apart. Line-wrapped pointers add more of the same — "Detect my radio", "Test CAT" and
//      "Re-run setup…" are real controls that are button text, not `settings-label` spans.
// Sixteen control names in the whole corpus are even reachable this way. A check with a failing
// control, ten false positives and no true ones is broken, not strict. Closing this gap properly
// needs the panel's controls to become declared data the way sections did — not a better regex.
// ---------------------------------------------------------------------------
