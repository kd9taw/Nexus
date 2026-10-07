// THE SHARED LIST'S VALIDATOR (features/sharedPanes). One source for what a box may show, every
// entry classified, and no entry that could be a stop control or a sender: each id passes the stop
// line's name backstop and is no cockpit's own id. Every rule is proven to FIRE on a planted list
// below, so a green run is a reading, not a blind pass.
import { describe, expect, it } from 'vitest'
import * as sharedPanes from './sharedPanes'
import { SHARED_PANES, type SharedPane } from './sharedPanes'
import { ALL_PANEL_VOCABULARIES, PHONE_PANELS, STOP_CONTROL_WORDS } from './panelState'
import { PANE_IDS } from './connectConfig'
import { paneById } from '../components/connect/panes'

const norm = (s: string) => s.toLowerCase().replace(/[^a-z]/g, '')

/** Every cockpit's own ids (and Connect's and the rail's slots), each with the views that hold it. */
function cockpitIds(): Map<string, string[]> {
  const out = new Map<string, string[]>()
  for (const v of ALL_PANEL_VOCABULARIES) for (const id of v.panelIds) out.set(id, [...(out.get(id) ?? []), v.view])
  return out
}

/** Everything wrong with a list, by the module header's rules. Empty is a valid list. */
function problems(list: readonly SharedPane[]): string[] {
  const out: string[] = []
  const own = cockpitIds()
  const ids = new Set<string>()
  const listed = new Map<string, number>()
  for (const e of list) {
    if (ids.has(e.id)) out.push(`"${e.id}" is listed twice`)
    ids.add(e.id)
    listed.set(e.pane, (listed.get(e.pane) ?? 0) + 1)
    if (!paneById(e.pane)) out.push(`"${e.id}" shows "${e.pane}", which is not a Conditions box`)
    if ((STOP_CONTROL_WORDS as readonly string[]).includes(norm(e.id))) out.push(`"${e.id}" is named for a stop control`)
    const views = own.get(e.id)
    if (views) out.push(`"${e.id}" is a cockpit's own id (${views.join(', ')})`)
    else if (e.id !== e.pane && !own.has(e.pane)) out.push(`"${e.id}" is renamed, but its Conditions id "${e.pane}" collides with nothing`)
    const id = e.id // the role branches below narrow `e` to never on a planted entry with no role
    if (e.role === 'fill') {
      if (!(typeof e.weight === 'number' && Number.isFinite(e.weight) && e.weight > 0)) out.push(`"${id}" fills with no weight`)
    } else if (e.role === 'content') {
      if ('weight' in e) out.push(`"${id}" takes its own height but carries a weight`)
    } else out.push(`"${id}" has no role`)
    if (typeof e.remote !== 'boolean') out.push(`"${e.id}" does not say whether the Remote page has it`)
  }
  for (const p of PANE_IDS) {
    const n = listed.get(p) ?? 0
    if (n !== 1) out.push(`the Conditions box "${p}" is listed ${n} times`)
  }
  return out
}

describe('the shared list', () => {
  it('lists every Conditions box exactly once, classified, and named apart from every cockpit pane', () => {
    expect(SHARED_PANES.length, 'the list is empty: every check below would be reading nothing').toBeGreaterThan(0)
    expect(ALL_PANEL_VOCABULARIES.length, 'no vocabularies to check the ids against').toBeGreaterThanOrEqual(9)
    expect(problems(SHARED_PANES)).toEqual([])
  })

  it('keeps its Conditions id, except the three that a cockpit already uses for something of its own', () => {
    // `scope` is every cockpit's spectrum strip, `activity` JS8's decode window and `spots` Phone's and
    // CW's own Spots pane: a box recording one of those would read as that cockpit's pane.
    const renamed = SHARED_PANES.filter((e) => e.id !== e.pane).map((e) => [e.pane, e.id])
    expect(renamed).toEqual([
      ['activity', 'activityMatrix'],
      ['scope', 'bandScope'],
      ['spots', 'spotsBoard'],
    ])
  })

  it('FIRES: each rule names its planted breach', () => {
    // Planted on Spots, whose Conditions id a cockpit does use, so a planted id of its own is a legal
    // rename and every message below is the one rule it was planted for.
    const fill = { pane: 'spots', role: 'fill', weight: 1, remote: true }
    const planted = [
      ...SHARED_PANES.filter((e) => e.pane !== 'spots'),
      { ...fill, id: 'spotsBoard' },
      { ...fill, id: 'spotsBoard' },
      { ...fill, id: 'ptt' },
      { ...fill, id: 'stopTx' },
      { ...fill, id: 'voiceKeyer' },
      { ...fill, id: 'txmsgs' },
      { ...fill, id: 'spots' },
      { id: 'noWeight', pane: 'spots', role: 'fill', remote: true },
      { id: 'heavy', pane: 'spots', role: 'content', weight: 2, remote: true },
      { id: 'noRole', pane: 'spots', remote: true },
      { id: 'unsure', pane: 'spots', role: 'fill', weight: 1 },
      { id: 'ghost', pane: 'ghost', role: 'content', remote: true },
      { id: 'clockFace', pane: 'clock', role: 'content', remote: true },
    ] as unknown as SharedPane[]
    expect(problems(planted)).toEqual([
      '"spotsBoard" is listed twice',
      '"ptt" is named for a stop control',
      '"stopTx" is named for a stop control',
      '"voiceKeyer" is a cockpit\'s own id (phone)',
      '"txmsgs" is a cockpit\'s own id (operate)',
      '"spots" is a cockpit\'s own id (phone, cw)',
      '"noWeight" fills with no weight',
      '"heavy" takes its own height but carries a weight',
      '"noRole" has no role',
      '"unsure" does not say whether the Remote page has it',
      '"ghost" shows "ghost", which is not a Conditions box',
      '"clockFace" is renamed, but its Conditions id "clock" collides with nothing',
      'the Conditions box "clock" is listed 2 times',
      'the Conditions box "spots" is listed 11 times',
    ])
    // A Conditions box the list forgot is caught as well.
    expect(problems(SHARED_PANES.filter((e) => e.pane !== 'pota'))).toEqual(['the Conditions box "pota" is listed 0 times'])
  })

  it('is not shaped like a panel vocabulary, so the export scan in panelState.test.ts stays honest', () => {
    // panelState.test.ts finds every vocabulary by this shape and holds the stop line's name backstop to
    // all of them. The list says what a box may SHOW, not what a cockpit may hide; were it vocabulary-
    // shaped, that scan would count it, and a cockpit's sweep could be driven off it.
    const isVocab = (v: unknown) =>
      !!v &&
      typeof v === 'object' &&
      typeof (v as { view?: unknown }).view === 'string' &&
      Array.isArray((v as { panelIds?: unknown }).panelIds)
    expect(isVocab(PHONE_PANELS), 'the shape test recognises nothing: it would pass anything').toBe(true)
    expect(Object.entries(sharedPanes).filter(([, v]) => isVocab(v)).map(([k]) => k)).toEqual([])
    expect(SHARED_PANES.filter(isVocab)).toEqual([])
  })
})
