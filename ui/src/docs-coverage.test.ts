// DOES THE MANUAL COVER WHAT WE SHIPPED?
//
// Every other doc guard in this tree checks that documentation which EXISTS is correct:
// `docs-match-code` checks quoted values, `docs-settings-pointers` checks routes,
// `gen-settings-reference --check` and `gen-wiki --check` check generated pages are current.
// All of them are green when a capability has NO documentation at all, because there is no
// page for them to be wrong about. That is the hole this file closes.
//
// It is not hypothetical. PSK31 and QPSK31 shipped as the headline of 1.7.0 — a whole
// operating section with two sub-modes, a cockpit, transmit and receive — and the manual
// never gained a chapter. Nothing went red: not CI, not the release-docs pass, not the site
// build. It was found by an operator reading the website and asking where PSK was, four
// releases later. macOS went the same way on the site side: a shipped platform that four
// separate platform lists said nothing about.
//
// THE RULE: a shipped section is a promise to explain it. If `features/registry.ts` — the
// single source of truth for what Nexus offers — lists a section, the manual owes it a
// chapter, and the guide index owes that chapter a link. A capability nobody can read about
// is, from the operator's chair, a capability that does not exist, which is the same
// reasoning the settings registry and its 228 deep links were built on.
//
// WHY THIS IMPORTS THE REGISTRY INSTEAD OF PARSING IT. The first version of this check
// regex-scanned registry.ts and found FIVE sections; there are eight. A parser that silently
// under-reports turns this guard into a green light for the exact gap it exists to catch, so
// the list comes from the module itself and cannot drift from what the app ships.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { sectionFeatures } from './features/registry'

const guideDir = fileURLToPath(new URL('../../docs/guide/', import.meta.url))
const docsDir = fileURLToPath(new URL('../../docs/', import.meta.url))

/** Sections whose chapter is deliberately named something else, each with the reason.
 *  An alias is a decision, not an escape hatch: adding one says "this IS documented, under
 *  this name", and the file still has to exist. */
const CHAPTER_ALIASES: Record<string, string> = {
  // POTA/SOTA is documented together with contesting because an activation and a contest are
  // the same operating problem in this app — a run rate, a log, and a spot.
  pota: 'contesting-pota',
  // Field Day shares that chapter for the same reason; it is that chapter's first sentence.
  fieldDay: 'contesting-pota',
  // The Operate cockpit's chapter is named for what it operates, not for the nav entry.
  operate: 'operate-digital',
  // The logbook chapter covers QSLing in the same breath, because in this app they are the
  // same screen.
  logbook: 'logbook-qsl',
  // Settings is documented as a reference rather than a walkthrough — it is generated from
  // the settings registry, which is why it is the one chapter that cannot drift.
  settings: 'settings-reference',
  // The remaining four are plain naming: the nav id is short, the chapter is spelled out.
  needed: 'needed-dx',
  dxped: 'dxpeditions',
  sats: 'satellites',
  awards: 'awards-journey',
}

/** Sections we KNOW have no chapter, each with why and what it is waiting on.
 *
 *  An entry here is a debt that has been written down, not a gap that has been forgiven —
 *  and it cannot go stale in either direction. Leave a gap out and the test fails, which is
 *  the point. FILL the gap without removing the entry and the test ALSO fails, because the
 *  chapter now exists and this list says it does not. So the list can only be correct. */
const KNOWN_GAPS: Record<string, string> = {
  // Tempo's chat section. `docs/Tempo-Protocol.md` and the product brief describe the
  // protocol and the product; neither is an operator chapter about using the screen. Found
  // 2026-08-20 by this guard, alongside the PSK gap that prompted it.
  chat: 'Tempo chat has protocol docs but no operator chapter — owed.',
}

const chapters = () =>
  readdirSync(guideDir)
    .filter((f) => f.endsWith('.md'))
    .map((f) => f.replace(/\.md$/, ''))

describe('the manual covers what the app ships', () => {
  it('control: the registry and the guide are both actually readable', () => {
    // Without this, an empty read makes every assertion below vacuously true — the failure
    // mode that matters most in a coverage check, because it reports total success.
    expect(sectionFeatures().length, 'no sections read from the registry').toBeGreaterThan(5)
    expect(chapters().length, 'no chapters read from docs/guide').toBeGreaterThan(10)
  })

  it('every section in the feature registry has a chapter of its own', () => {
    const have = new Set(chapters())
    const missing = sectionFeatures()
      .filter((f) => !(f.id in KNOWN_GAPS))
      .map((f) => ({ id: f.id, file: CHAPTER_ALIASES[f.id] ?? f.id }))
      .filter((s) => !have.has(s.file))
      .map((s) => `${s.id} → docs/guide/${s.file}.md`)
    expect(
      missing,
      `A shipped section with no chapter. Write docs/guide/<id>.md and link it from\n` +
        `docs/guide/index.md, add an entry to CHAPTER_ALIASES saying where it lives, or —\n` +
        `if it genuinely is not written yet — record it in KNOWN_GAPS with the reason:\n  ` +
        missing.join('\n  '),
    ).toEqual([])
  })

  it('a recorded gap disappears from the list the moment somebody fills it', () => {
    // The half that stops KNOWN_GAPS becoming a place where debts are quietly parked
    // forever: once the chapter exists, this fails until the entry is deleted.
    const have = new Set(chapters())
    const filled = Object.keys(KNOWN_GAPS)
      .map((id) => ({ id, file: CHAPTER_ALIASES[id] ?? id }))
      .filter((g) => have.has(g.file))
      .map((g) => g.id)
    expect(
      filled,
      `docs/guide/ now has a chapter for these, so remove them from KNOWN_GAPS: ${filled.join(', ')}`,
    ).toEqual([])
  })

  it('every recorded gap names a section that still exists', () => {
    // A gap for a section that has since been renamed or removed is a note about nothing.
    const ids = new Set(sectionFeatures().map((f) => f.id))
    const stale = Object.keys(KNOWN_GAPS).filter((id) => !ids.has(id))
    expect(stale, `KNOWN_GAPS names sections the registry no longer has: ${stale.join(', ')}`).toEqual([])
  })

  it('every chapter is reachable from the guide index', () => {
    const index = readFileSync(`${guideDir}index.md`, 'utf8')
    const orphans = chapters()
      .filter((c) => c !== 'index')
      .filter((c) => !index.includes(`(${c}.md)`))
    expect(
      orphans,
      `A chapter nothing links to is a chapter nobody finds:\n  ${orphans.join('\n  ')}`,
    ).toEqual([])
  })

  it('every platform we publish an installer for is named in install.md', () => {
    // The macOS case: it shipped at 1.5.0 as a signed, notarised build in every release, and
    // the places that tell an operator it exists were written before it did. install.md is
    // the one page whose whole job is "how do I get this on my machine".
    const install = readFileSync(`${docsDir}install.md`, 'utf8')
    const PLATFORMS = ['Windows', 'macOS', 'Linux', 'Raspberry Pi']
    const unnamed = PLATFORMS.filter((p) => !install.includes(p))
    expect(unnamed, `install.md never mentions: ${unnamed.join(', ')}`).toEqual([])
  })

  it('control: the coverage checks can actually fail', () => {
    // Each assertion above is a "nothing found" result, and this project's most-repeated
    // defect is trusting one of those without proving the check fires. A section id that
    // cannot have a chapter must be reported as missing; a chapter nothing links to must be
    // reported as an orphan.
    const have = new Set(chapters())
    expect(have.has('no-such-section'), 'a bogus id must not resolve to a chapter').toBe(false)
    expect(existsSync(`${guideDir}no-such-section.md`)).toBe(false)
    const index = readFileSync(`${guideDir}index.md`, 'utf8')
    expect(index.includes('(no-such-section.md)'), 'the index link test can miss').toBe(false)
  })
})
