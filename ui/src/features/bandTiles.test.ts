// BANDS FOR YOU — the tile model (features/bandTiles.ts): band-stack order, the list's word and colour
// (the colour follows the word), the heard dot, the advisor's best band, the rig's band, the VHF
// opening's mode, and hollow tiles for data that is missing, offline or stale — never green.
import { describe, it, expect } from 'vitest'
import { bandTiles, TILE_BANDS } from './bandTiles'
import { BAND_CONDITIONS_STALE_S } from '../bandConditions'
import type { BandReport, OpeningView, PropagationSnapshot } from '../types'

const NOW = 1_800_000_000
const report = (band: string, modeled: BandReport['modeled'], tier: BandReport['tier']): BandReport =>
  ({ band, modeled, tier, score: 0.1, nHearMe: 0, nIHear: 0, bestRegion: null, confidence: 'Likely', reason: 'r' }) as BandReport
const opening = (band: string, mode: string): OpeningView => ({ band, mode }) as OpeningView
function snap(bands: BandReport[], over: Partial<PropagationSnapshot> = {}): PropagationSnapshot {
  return { advisory: { headline: '', banners: [], bands }, openings: [], source: 'live', asOf: NOW - 60, ...over } as unknown as PropagationSnapshot
}
const view = (tiles: ReturnType<typeof bandTiles>) => tiles.map((t) => [t.band, t.state, t.word, t.color, t.heard, t.best, t.onRig, t.mode])

describe('the band tiles', () => {
  it('stand in band-stack order, whatever order the advisor ranks them in', () => {
    const ranked = [report('20m', 'Open', 'Active'), report('160m', 'Closed', 'Closed'), report('40m', 'Open', 'Quiet'), report('6m', 'Closed', 'Closed')]
    expect(bandTiles(snap(ranked), null, NOW).map((t) => t.band)).toEqual(['160m', '40m', '20m', '6m'])
    expect(TILE_BANDS.indexOf('160m')).toBeLessThan(TILE_BANDS.indexOf('2m'))
  })

  it('say the list’s word in its colour: a band heard now is Open and green, whatever the model says', () => {
    const tiles = bandTiles(
      snap([report('10m', 'Closed', 'Active'), report('17m', 'Marginal', 'Moderate'), report('20m', 'Open', 'Quiet'), report('30m', 'Marginal', 'Quiet'), report('40m', 'Closed', 'Quiet')]),
      null,
      NOW,
    )
    expect(view(tiles)).toEqual([
      ['40m', 'closed', 'Closed', 'var(--band-closed)', 'none', false, false, null],
      ['30m', 'marginal', 'Marginal', 'var(--band-marginal)', 'none', false, false, null],
      ['20m', 'open', 'Open', 'var(--band-open)', 'none', false, false, null],
      ['17m', 'open', 'Open', 'var(--band-open)', 'some', false, false, null],
      ['10m', 'open', 'Open', 'var(--band-open)', 'active', true, false, null],
    ])
  })

  it('star the advisor’s best band: the first of its ranking that is not closed', () => {
    const tiles = bandTiles(snap([report('10m', 'Closed', 'Quiet'), report('15m', 'Marginal', 'Quiet'), report('20m', 'Open', 'Active')]), null, NOW)
    expect(tiles.filter((t) => t.best).map((t) => t.band)).toEqual(['15m'])
    expect(bandTiles(snap([report('10m', 'Closed', 'Quiet')]), null, NOW).some((t) => t.best), 'nothing open, nothing starred').toBe(false)
  })

  it('ring the band the radio is on', () => {
    const tiles = bandTiles(snap([report('20m', 'Open', 'Quiet'), report('40m', 'Open', 'Quiet')]), '40m', NOW)
    expect(tiles.filter((t) => t.onRig).map((t) => t.band)).toEqual(['40m'])
  })

  it('name a VHF opening’s mode, and show 4 m or 2 m only when there is something there', () => {
    const p = snap([report('6m', 'Closed', 'Active'), report('10m', 'Open', 'Quiet')], {
      openings: [opening('6m', 'Sporadic-E'), opening('2m', 'Tropo'), opening('10m', 'F2'), opening('4m', 'Unknown')],
    })
    const tiles = bandTiles(p, null, NOW)
    expect(tiles.map((t) => [t.band, t.mode])).toEqual([
      ['10m', null], // HF: no mode chip, the opening detector's HF call is the model's business
      ['6m', 'Es'],
      ['2m', 'Tropo'],
    ])
    // 2 m has no report of its own: the opening is a band heard open.
    expect(view(tiles.filter((t) => t.band === '2m'))).toEqual([['2m', 'open', 'Open', 'var(--band-open)', 'active', false, false, 'Tropo']])
  })

  it('are hollow and neutral, never green, with no data, offline data or stale data', () => {
    const bands = [report('20m', 'Open', 'Active')]
    for (const [why, p] of [
      ['no snapshot', null],
      ['offline', snap(bands, { source: 'offline' })],
      ['stale', snap(bands, { asOf: NOW - BAND_CONDITIONS_STALE_S - 1 })],
    ] as const) {
      const tiles = bandTiles(p, '20m', NOW)
      expect(tiles.length, why).toBe(11)
      expect(tiles.every((t) => t.state === 'unknown' && t.color === null && !t.best), why).toBe(true)
      expect(tiles.find((t) => t.band === '20m')!.onRig, `${why}: the radio's band is still ringed`).toBe(true)
    }
    // CONTROL: the same bands, fresh, are coloured.
    expect(bandTiles(snap(bands), null, NOW)[0].color).toBe('var(--band-open)')
  })
})
