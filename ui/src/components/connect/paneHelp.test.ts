// A PANE'S "? … in the manual" LINK LANDS ON A HEADING THAT EXISTS — computed against the markdown
// the published manual is built from (docs/guide/), never trusted from the table in paneHelp.ts.
//
// The site renders docs/guide/<chapter>.md at hamradiotools.io/manual/<chapter> with GitHub's heading
// ids (read off the built site, 2026-09-30: `the-pane-grid`, `chase-whats-workable-now`, …), so a
// heading renamed in the guide turns a pane's link into one that opens at the top of the chapter.
// This fails first. The slug rule is release-docs.mjs's own (`slugify` / `anchorsOf`, which checks
// every in-doc link the same way): inline markup stripped, lowercase, punctuation dropped, spaces to
// hyphens, and GitHub's `-1`, `-2` for a repeated heading.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { MANUAL_BASE, PANE_HELP, paneHelpUrl } from './paneHelp'
import { paneById } from './panes'
import { PANE_IDS, type PaneId } from '../../features/connectConfig'

const slugify = (heading: string) =>
  heading
    .replace(/`([^`]*)`/g, '$1')
    .replace(/\*\*?([^*]*)\*\*?/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .trim()
    .toLowerCase()
    .replace(/[^\w\- ]/gu, '')
    .replace(/ /g, '-')

/** Every heading id in a chapter, fenced code skipped (release-docs.mjs `anchorsOf`). */
function anchorsOf(chapter: string): Set<string> {
  const text = readFileSync(resolve(process.cwd(), '..', 'docs', 'guide', `${chapter}.md`), 'utf8')
  const seen = new Map<string, number>()
  const out = new Set<string>()
  let fence: string | null = null
  for (const line of text.split('\n')) {
    const f = /^\s*(```+|~~~+)/.exec(line)
    if (f) {
      if (fence === null) fence = f[1][0]
      else if (f[1][0] === fence) fence = null
      continue
    }
    if (fence !== null) continue
    const m = /^#{1,6}\s+(.*)$/.exec(line)
    if (!m) continue
    const base = slugify(m[1])
    const n = seen.get(base) ?? 0
    seen.set(base, n + 1)
    out.add(n === 0 ? base : `${base}-${n}`)
  }
  return out
}

/** The panes the manual does not describe yet — no row in the Connect chapter's pane table and no
 *  section of their own. Named here so a new pane cannot go without a decision: add it to the manual
 *  and to PANE_HELP, or add it to this list. None today: the last five got their rows (2026-09-30). */
const NOT_IN_THE_MANUAL: PaneId[] = []

describe('a pane’s manual link', () => {
  it('the slug rule is the published site’s (control: headings whose ids were read off the built manual)', () => {
    const ids = anchorsOf('connect')
    for (const id of ['the-pane-grid', 'the-amplifier-pane', 'read-an-opening', 'chase-whats-workable-now', 'track-propagation-to-a-specific-call'])
      expect(ids.has(id), id).toBe(true)
    // …and the check can fail: an id no heading produces is not there.
    expect(ids.has('the-panes-grid')).toBe(false)
    expect(slugify("Chase what's workable now"), 'an apostrophe is dropped, not hyphenated').toBe('chase-whats-workable-now')
  })

  it('every link lands on a heading that exists in its chapter', () => {
    const missing: string[] = []
    for (const [id, h] of Object.entries(PANE_HELP)) {
      if (!anchorsOf(h!.chapter).has(h!.anchor)) missing.push(`${id} → ${h!.chapter}#${h!.anchor}`)
    }
    expect(Object.keys(PANE_HELP).length, 'the table is not empty').toBeGreaterThan(15)
    expect(missing, 'these links would open at the top of the chapter').toEqual([])
  })

  it('is the published page, #section', () => {
    expect(paneHelpUrl('amp')).toBe('https://hamradiotools.io/manual/connect#the-amplifier-pane')
    expect(paneHelpUrl('spacewx')).toBe(`${MANUAL_BASE}connect#the-pane-grid`)
    expect(paneHelpUrl('bandTiles'), 'the pane in the default layout has its row').toBe(`${MANUAL_BASE}connect#the-pane-grid`)
    expect(paneHelpUrl('nope' as PaneId), 'no manual text, no link').toBeNull()
  })

  it('every pane either has a link or is named as not in the manual yet — never both, never neither', () => {
    const linked = PANE_IDS.filter((id) => PANE_HELP[id])
    const unlinked = PANE_IDS.filter((id) => !PANE_HELP[id])
    expect(unlinked.sort()).toEqual([...NOT_IN_THE_MANUAL].sort())
    expect(linked.length + unlinked.length).toBe(PANE_IDS.length)
  })

  it('every pane linked to the pane grid has its row in the Connect chapter’s pane table, and a pane named as not in the manual has none', () => {
    // The table's first column is each pane's English name: a link to the pane grid for a pane with no
    // row there would open a page that does not mention it.
    const text = readFileSync(resolve(process.cwd(), '..', 'docs', 'guide', 'connect.md'), 'utf8')
    const rows = new Set([...text.matchAll(/^\| ([^|]+?) \| [^|]+ \|$/gm)].map((m) => m[1].trim()))
    expect(rows.has('Space Wx'), 'control: the table was read').toBe(true)
    const gridPanes = PANE_IDS.filter((id) => PANE_HELP[id]?.anchor === 'the-pane-grid')
    expect(gridPanes.filter((id) => !rows.has(paneById(id)!.title)), 'linked to the pane grid, with no row in it').toEqual([])
    for (const id of NOT_IN_THE_MANUAL) expect(rows.has(paneById(id)!.title), id).toBe(false)
  })
})
