// @vitest-environment jsdom
//
// NO MODEL, NOBODY HEARD: UNKNOWN, NEVER OPEN. With no usable solar flux and Kp the station runs no
// band model, and each band's `modeled` is absent from its report (an older station that sends none
// reads the same). A band nobody heard then has no evidence either way: every surface must say it
// is unknown — the "—" word, a hollow dot, no best-band star — and never "Open".
//
// It used to read "Open · none heard" on every one, because the word fell back to Open for a
// missing model: the Band conditions list, the Band Advisor's rows, the band menu's dots, the NOW
// bar, the tiles, the best band, the two Basic lines and the best-band table all said Open, for
// 160 m to 70 cm alike. The controls: a band heard now still reads Open, and a model word, when
// there is one, is still shown.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { BandConditionStrip } from './BandConditionStrip'
import { BandAdvisor } from './BandAdvisor'
import { BestBandTable } from './BestBandTable'
import { BandTiles } from './BandTiles'
import { NowBar } from '../NowBar'
import { publishBandConditions, useBandConditions } from '../../bandConditions'
import { bandConditionCell, dualStateLabel, NO_MODEL_WORD } from '../../propViz'
import { bandTiles } from '../../features/bandTiles'
import { bandAdvisorLine, bestbandLine } from '../connect/paneFormat'
import type { PaneContext } from '../connect/paneContext'
import type { AppSnapshot, BandReport, PropagationSnapshot, RegionBest } from '../../types'

const NOW = Math.floor(Date.now() / 1000)
const report = (band: string, tier: BandReport['tier'], modeled?: BandReport['modeled']): BandReport =>
  ({ band, tier, modeled, score: 0.1, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: 'r' }) as BandReport
const SILENT = ['160m', '80m', '40m', '20m', '10m', '6m', '2m', '70cm']
/** Every band silent, half Quiet and half Closed, none with a model word. */
const NO_MODEL = SILENT.map((b, i) => report(b, i % 2 ? 'Closed' : 'Quiet'))
const prop = (bands: BandReport[], bestToRegion: RegionBest[] = []) =>
  ({
    advisory: { headline: '', banners: [], bands },
    openings: [],
    dxpeditions: { workableNow: [], active: [], upcoming: [] },
    bestToRegion,
    source: 'partial',
    asOf: NOW,
  }) as unknown as PropagationSnapshot
const region = (band: string, tier: BandReport['tier'], modeled?: BandReport['modeled']): RegionBest =>
  ({ region: 'Japan', octant: 'NW', bearingDeg: 320, band, tier, modeled, stations: 2, bidirectional: false, score: 0.2 }) as RegionBest
// Unanchored: the components run words together ("20mOpen2").
const OPEN = /Open/

afterEach(() => {
  cleanup()
  publishBandConditions(null)
})

describe('a silent band with no model word is unknown, never Open', () => {
  it('the word: "—" for Quiet and Closed alike, with the model word or a heard band as controls', () => {
    for (const tier of ['Quiet', 'Closed'] as const) {
      expect(dualStateLabel(undefined, tier).word).toBe(NO_MODEL_WORD)
    }
    for (const r of NO_MODEL) {
      const cell = bandConditionCell(r)
      expect([r.band, cell.word]).toEqual([r.band, NO_MODEL_WORD])
      expect(cell.color, 'unknown is never green').not.toBe('var(--band-open)')
    }
    // Controls: heard is Open with or without a model; a model word is passed through.
    expect(dualStateLabel(undefined, 'Active').word).toBe('Open')
    expect(dualStateLabel(undefined, 'Moderate').word).toBe('Open')
    expect(dualStateLabel('Open', 'Quiet').word).toBe('Open')
    expect(dualStateLabel('Closed', 'Quiet').word).toBe('Closed')
  })

  it('the Band conditions list, the Band Advisor and the best-band table say no "Open"', () => {
    const strip = render(<BandConditionStrip bands={NO_MODEL} />).container
    expect(strip.textContent).not.toMatch(OPEN)
    expect(strip.textContent).toContain(NO_MODEL_WORD)
    cleanup()
    const advisor = render(<BandAdvisor bands={NO_MODEL} />).container
    expect(advisor.querySelectorAll('.ba-row').length).toBe(SILENT.length)
    expect(advisor.textContent).not.toMatch(OPEN)
    cleanup()
    const table = render(<BestBandTable rows={[region('20m', 'Quiet'), region('15m', 'Closed')]} />).container
    expect(table.textContent).not.toMatch(OPEN)
    cleanup()
    // Control: a heard band in the same table is Open.
    expect(render(<BestBandTable rows={[region('20m', 'Active')]} />).container.textContent).toMatch(OPEN)
  })

  it('the band menu draws a hollow unknown dot and the NOW bar no Open', () => {
    publishBandConditions(prop(NO_MODEL))
    let lookup: ReturnType<typeof useBandConditions> | null = null
    function Probe() {
      lookup = useBandConditions()
      return null
    }
    render(<Probe />)
    for (const band of SILENT) {
      const c = lookup!(band)
      expect([band, c.state, c.color]).toEqual([band, 'unknown', null])
      expect(c.word).not.toMatch(OPEN)
      expect(c.title).not.toMatch(OPEN)
    }
    cleanup()
    const snap = { radio: { band: '20m' } } as unknown as AppSnapshot
    const bar = render(
      <NowBar snap={snap} prop={prop(NO_MODEL)} feedHealth={null} connectEnabled={false} dxpedEnabled={false} onNavigate={() => {}} />,
    ).container
    expect(bar.textContent).not.toMatch(OPEN)
    cleanup()
    // Control: a heard band in the same snapshot is open in the menu.
    publishBandConditions(prop([...NO_MODEL, report('15m', 'Active')]))
    render(<Probe />)
    expect(lookup!('15m').state).toBe('open')
  })

  it('the tiles: unknown, hollow, and none is the best band', () => {
    const tiles = bandTiles(prop(NO_MODEL), null, NOW)
    expect(tiles.length).toBeGreaterThan(0)
    for (const tile of tiles) {
      expect([tile.band, tile.state, tile.color, tile.best]).toEqual([tile.band, 'unknown', null, false])
      expect(tile.word).not.toMatch(OPEN)
    }
    const drawn = render(<BandTiles prop={prop(NO_MODEL)} rigBand={null} />).container
    expect(drawn.textContent).not.toMatch(OPEN)
    // Control: a heard band is open and is the best one.
    const heard = bandTiles(prop([...NO_MODEL, report('15m', 'Active')]), null, NOW).find((t) => t.band === '15m')!
    expect([heard.state, heard.best]).toEqual(['open', true])
  })

  it('the Basic band and best-band lines name no Open band', () => {
    const ctx = (p: PropagationSnapshot) => ({ prop: p }) as unknown as PaneContext
    const line = bandAdvisorLine(ctx(prop(NO_MODEL)))
    expect(line).not.toMatch(OPEN)
    expect(line).not.toMatch(/160m/)
    const best = bestbandLine(ctx(prop(NO_MODEL, [region('20m', 'Quiet')])))
    expect(best).not.toMatch(OPEN)
    expect(best).toBe('To Japan: try 20m.')
    // Controls: a heard band is the Basic line's open band, and a model word is still quoted.
    expect(bandAdvisorLine(ctx(prop([...NO_MODEL, report('15m', 'Active')])))).toBe('Best band now: 15m (Open).')
    expect(bestbandLine(ctx(prop([], [region('20m', 'Quiet', 'Marginal')])))).toBe('To Japan: try 20m (Marginal).')
  })
})
