// OPERATE'S COLUMNS (layout L5): the pure geometry behind Classic's two column dividers and Roster's
// one. The rule every divider in the app keeps — it moves only its own two columns — is the
// property tested here, from each place a Classic record can start: the sheet, an earlier build's
// pair, and the dividers' own widths.
import { describe, it, expect } from 'vitest'
import {
  CLASSIC_FR,
  classicCommit,
  classicReset,
  classicScale,
  classicStyle,
  classicWidths,
  rosterStyle,
  type ClassicWidths,
} from './operateColumns'
import { emptyPanelLayout, seamShares, type OperatePanelId, type PanelLayout } from './panelState'

const layout = (over: Partial<PanelLayout<OperatePanelId>> = {}): PanelLayout<OperatePanelId> => ({
  ...emptyPanelLayout<OperatePanelId>(),
  ...over,
})
/** Each column's fraction of the grid. */
const fractions = (w: ClassicWidths) => {
  const total = w.fr[0] + w.fr[1] + w.fr[2]
  return w.fr.map((v) => v / total)
}
/** The widths a stored `{a, b}` reads back as. */
const stored = (cols: { a: number; b: number }) => classicWidths(layout({ v: 2, cols }))

describe('where Classic’s widths come from', () => {
  it('nothing stored: the sheet’s own, and nothing painted', () => {
    expect(classicWidths(layout())).toEqual({ fr: [...CLASSIC_FR], source: 'stock' })
    expect(classicStyle(layout())).toBeUndefined()
  })

  it('an earlier build’s pair opens exactly as that build painted it (Band Activity left to the sheet)', () => {
    const l = layout({ share: { txmsgs: 1.2, stations: 0.8 } })
    expect(classicWidths(l)).toEqual({ fr: [1.15, 1.2, 0.8], source: 'legacy' })
    expect(classicStyle(l)).toEqual({ '--op-col-a': '1.2fr', '--op-col-b': '0.8fr' })
    // Half a pair (a hand-edited record) paints its half, as before.
    expect(classicStyle(layout({ share: { stations: 0.5 } }))).toEqual({ '--op-col-b': '0.5fr' })
  })

  it('the dividers’ widths: fractions ×2 for the two edge columns, the Rx Frequency column the rest', () => {
    const l = layout({ v: 2, share: { txmsgs: 1.2, stations: 0.8 }, cols: { a: 0.8, b: 0.5 } })
    const w = classicWidths(l)
    expect(w.source, 'the dividers’ own widths win over an older pair').toBe('cols')
    expect(w.fr.map((v) => +v.toFixed(12))).toEqual([0.4, 0.35, 0.25])
    expect(classicStyle(l)).toEqual({ '--op-col-ba': '0.4fr', '--op-col-a': `${1 - 0.4 - 0.25}fr`, '--op-col-b': '0.25fr' })
  })

  it('a stored pair that leaves the Rx Frequency column nothing is not a layout: the earlier layers stand', () => {
    expect(classicWidths(layout({ v: 2, cols: { a: 1.2, b: 0.8 } })).source).toBe('stock')
    expect(classicWidths(layout({ v: 2, share: { txmsgs: 1 }, cols: { a: 1.85, b: 1.85 } })).source).toBe('legacy')
    expect(classicWidths(layout({ v: 2, cols: { a: 0.8 } })).source, 'half a pair').toBe('stock')
  })
})

describe('a Classic divider moves only its own two columns', () => {
  const starts: Array<[string, PanelLayout<OperatePanelId>]> = [
    ['the sheet', layout()],
    ['an earlier build’s pair', layout({ share: { txmsgs: 1.4, stations: 0.6 } })],
    ['the dividers’ widths', layout({ v: 2, cols: { a: 0.7, b: 0.4 } })],
  ]
  for (const [from, l] of starts) {
    it(`from ${from}: the Rx Frequency column / Stations divider never moves Band Activity`, () => {
      const w = classicWidths(l)
      for (const f of [0.2, 0.5, 0.8]) {
        const [av, bv] = seamShares(f)
        const after = stored(classicCommit(w, 1, 2, av, bv))
        expect(fractions(after)[0], `f=${f}`).toBeCloseTo(fractions(w)[0], 12)
        // And the pair splits the way the divider says.
        expect(after.fr[1] / (after.fr[1] + after.fr[2])).toBeCloseTo(f, 12)
      }
    })

    it(`from ${from}: the Band Activity / Rx Frequency column divider never moves Stations`, () => {
      const w = classicWidths(l)
      for (const f of [0.3, 0.55, 0.7]) {
        const [av, bv] = seamShares(f)
        const after = stored(classicCommit(w, 0, 1, av, bv))
        expect(fractions(after)[2], `f=${f}`).toBeCloseTo(fractions(w)[2], 12)
        expect(after.fr[0] / (after.fr[0] + after.fr[1])).toBeCloseTo(f, 12)
      }
    })
  }

  it('with the rail on the left, the Stations / Band Activity divider never moves the Rx Frequency column', () => {
    const w = classicWidths(layout({ v: 2, cols: { a: 0.8, b: 0.5 } }))
    const [av, bv] = seamShares(0.35)
    const after = stored(classicCommit(w, 2, 0, av, bv))
    expect(fractions(after)[1]).toBeCloseTo(fractions(w)[1], 12)
    expect(after.fr[2] / (after.fr[2] + after.fr[0])).toBeCloseTo(0.35, 12)
  })

  it('paints a pair at its own mean, so its painted total is the total it had', () => {
    const w = classicWidths(layout())
    expect(classicScale(w, 1, 2)).toBeCloseTo((0.95 + 0.72) / 2, 12)
    expect(classicScale(w, 0, 1)).toBeCloseTo((1.15 + 0.95) / 2, 12)
  })

  it('the far end of a drag is stored in the record’s range, with the Rx Frequency column kept', () => {
    const w = classicWidths(layout())
    const far = classicCommit(w, 0, 1, ...seamShares(1))
    expect(far.a).toBeLessThanOrEqual(2 - 0.15)
    expect(far.b).toBeGreaterThanOrEqual(0.15)
    const back = stored(far)
    expect(back.source).toBe('cols')
    expect(back.fr[1]).toBeGreaterThan(0)
  })
})

describe('a Classic divider’s reset', () => {
  it('puts only its own pair back to the sheet’s ratio; the third column keeps its width', () => {
    const w = classicWidths(layout({ v: 2, cols: { a: 0.9, b: 0.3 } }))
    const after = stored(classicReset(w, 1, 2)!)
    expect(fractions(after)[0]).toBeCloseTo(fractions(w)[0], 12)
    expect(after.fr[1] / after.fr[2]).toBeCloseTo(CLASSIC_FR[1] / CLASSIC_FR[2], 12)
  })

  it('stores nothing once the whole grid is the sheet’s own again', () => {
    expect(classicReset(classicWidths(layout()), 0, 1)).toBeNull()
    // Moved by one divider and reset by the same one: back to the sheet.
    const moved = stored(classicCommit(classicWidths(layout()), 1, 2, ...seamShares(0.7)))
    expect(classicReset(moved, 1, 2)).toBeNull()
  })
})

describe('Roster’s one divider', () => {
  it('paints nothing until moved, then the Call Roster’s share and the rail’s rest of 2', () => {
    expect(rosterStyle(layout())).toBeUndefined()
    expect(rosterStyle(layout({ share: { callRoster: 1.2 } }))).toEqual({ '--op-roster-a': '1.2fr', '--op-roster-b': `${2 - 1.2}fr` })
  })
})
