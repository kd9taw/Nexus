// @vitest-environment jsdom
//
// ONE-TIME favorites seeding. The operator's ask was "set some favorites …
// to the most recent active birds, one time, then allow users to change from
// there", and every test here defends one half of that sentence:
//
//  - ONE TIME. A marker is written the moment the seed runs, and nothing
//    re-seeds afterwards. An operator who unstars everything gets an empty
//    sky, not a resurrection.
//  - ACTIVE. Only birds the catalog positively calls alive, with a live
//    amateur transmitter, with elements, and with a workable pass over THIS
//    operator's grid. Anything less defers — starring blind is worse than
//    starring nothing.
import { describe, it, expect, beforeEach } from 'vitest'
import type { SatView } from '../types'
import {
  SAT_SEED_CAP,
  ackSatSeedNotice,
  rankSatSeedCandidates,
  satSeedNoticeOpen,
  satSeedRecord,
  seedSatFavorites,
} from './satSeed'
import { satChasingSet, toggleSatChasing } from './satChase'

const NOW = Math.floor(Date.now() / 1000)

type Bird = SatView['birds'][number]
const bird = (name: string, norad: number, over: Partial<Bird> = {}): Bird => ({
  name,
  norad,
  lat: 0,
  lon: 0,
  altKm: 500,
  footprintKm: 2000,
  track: [],
  status: 'alive',
  amateur: true,
  ...over,
})

const pass = (name: string, norad: number, maxElDeg: number, n = 0) => ({
  name,
  norad,
  aosUnix: NOW + 600 + n * 6000,
  losUnix: NOW + 1200 + n * 6000,
  maxElDeg,
  aosAzDeg: 100,
  losAzDeg: 260,
  status: 'alive',
})

const view = (over: Partial<SatView> = {}): SatView => ({
  tleAgeDays: 1,
  usableCount: 97,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW - 3600,
  tleSource: 'mirror',
  birds: [],
  passes: [],
  excluded: [],
  ...over,
})

beforeEach(() => {
  localStorage.clear()
})

describe('rankSatSeedCandidates — "most-workable ACTIVE birds"', () => {
  it('ranks by workable pass count, then by the best peak elevation', () => {
    const v = view({
      birds: [bird('SO-50', 27607), bird('RS-44', 44909), bird('AO-7', 7530)],
      passes: [
        pass('SO-50', 27607, 45, 0),
        pass('SO-50', 27607, 20, 1),
        pass('SO-50', 27607, 12, 2),
        pass('RS-44', 44909, 70, 0),
        pass('RS-44', 44909, 30, 1),
        pass('AO-7', 7530, 80, 0),
      ],
    })
    expect(rankSatSeedCandidates(v).map((c) => c.name)).toEqual(['SO-50', 'RS-44', 'AO-7'])
  })

  it('drops a bird the catalog does NOT call alive', () => {
    const v = view({
      birds: [bird('AO-85', 40967, { status: 'dead' }), bird('SO-50', 27607)],
      passes: [pass('AO-85', 40967, 70), pass('SO-50', 27607, 40)],
    })
    expect(rankSatSeedCandidates(v).map((c) => c.name)).toEqual(['SO-50'])
  })

  it('drops an alive bird with no live amateur transmitter (in orbit, silent)', () => {
    const v = view({
      birds: [bird('QUIET-1', 90001, { amateur: false }), bird('SO-50', 27607)],
      passes: [pass('QUIET-1', 90001, 70), pass('SO-50', 27607, 40)],
    })
    expect(rankSatSeedCandidates(v).map((c) => c.name)).toEqual(['SO-50'])
  })

  it('drops a bird with no status at all — an unknown bird is never starred blind', () => {
    const v = view({
      birds: [bird('MYSTERY', 90002, { status: null, amateur: undefined })],
      passes: [pass('MYSTERY', 90002, 70)],
    })
    expect(rankSatSeedCandidates(v)).toEqual([])
  })

  it('ignores grazing passes — a 5° scrape is not a workable pass', () => {
    const v = view({
      birds: [bird('GRAZER', 90003), bird('SO-50', 27607)],
      passes: [pass('GRAZER', 90003, 5, 0), pass('GRAZER', 90003, 4, 1), pass('SO-50', 27607, 25)],
    })
    expect(rankSatSeedCandidates(v).map((c) => c.name)).toEqual(['SO-50'])
  })

  it('caps the set', () => {
    const birds = []
    const passes = []
    for (let i = 0; i < SAT_SEED_CAP + 6; i++) {
      birds.push(bird(`BIRD-${String(i).padStart(2, '0')}`, 90100 + i))
      passes.push(pass(`BIRD-${String(i).padStart(2, '0')}`, 90100 + i, 40))
    }
    expect(rankSatSeedCandidates(view({ birds, passes })).length).toBe(SAT_SEED_CAP)
  })
})

describe('seedSatFavorites — once, ever', () => {
  const good = () =>
    view({
      birds: [bird('SO-50', 27607), bird('RS-44', 44909)],
      passes: [pass('SO-50', 27607, 45), pass('RS-44', 44909, 60)],
    })

  it('stars the ranked birds on an empty first run and records the seed', () => {
    const rec = seedSatFavorites(good(), true)
    expect(rec?.names).toEqual(['RS-44', 'SO-50'])
    expect(satChasingSet()).toEqual(new Set(['RS-44', 'SO-50']))
    expect(satSeedRecord()?.names.length).toBe(2)
  })

  it('NEVER seeds twice — the marker survives an operator who unstars them all', () => {
    seedSatFavorites(good(), true)
    for (const n of [...satChasingSet()]) toggleSatChasing(n)
    expect(satChasingSet().size).toBe(0)
    expect(seedSatFavorites(good(), true)).toBeNull()
    expect(satChasingSet().size).toBe(0) // an empty sky, not a resurrection
  })

  it('never seeds over an operator who already has ★ birds', () => {
    toggleSatChasing('AO-91', 43017)
    expect(seedSatFavorites(good(), true)).toBeNull()
    expect(satChasingSet()).toEqual(new Set(['AO-91']))
    expect(satSeedRecord()).toBeNull()
  })

  it('never seeds over a star set on this build that was then cleared', () => {
    // The name→NORAD record survives an unstar (satChase.ts). Seeding over it
    // would be the resurrection the marker exists to prevent, one build late.
    toggleSatChasing('AO-91', 43017)
    toggleSatChasing('AO-91', 43017)
    expect(satChasingSet().size).toBe(0)
    expect(seedSatFavorites(good(), true)).toBeNull()
    expect(satChasingSet().size).toBe(0)
  })

  it('never seeds over an UPGRADING operator who cleared their stars', () => {
    // The real shape of the case: every operator upgrading into this build
    // has an EMPTY name→NORAD map, because that key ships with the seed. What
    // their shipped build did leave behind is the ★ list key itself — only a
    // toggle ever writes it, so an empty array is a list that was emptied on
    // purpose. Reading only the NORAD map re-stars ten birds for every one of
    // them.
    localStorage.setItem('nexus.sats.chasing', '[]')
    expect(localStorage.getItem('nexus.sats.chasingNorad')).toBeNull()
    expect(seedSatFavorites(good(), true)).toBeNull()
    expect(satChasingSet().size).toBe(0)
    expect(satSeedRecord()).toBeNull()
  })

  it('DEFERS without a grid — no marker, so it still seeds once one exists', () => {
    expect(seedSatFavorites(good(), false)).toBeNull()
    expect(satSeedRecord()).toBeNull() // deferred, NOT spent
    expect(seedSatFavorites(good(), true)?.names.length).toBe(2)
  })

  it('DEFERS with no view and with no candidate — never spends the one seed on nothing', () => {
    expect(seedSatFavorites(null, true)).toBeNull()
    expect(seedSatFavorites(view(), true)).toBeNull() // catalog not landed yet
    expect(satSeedRecord()).toBeNull()
    expect(seedSatFavorites(good(), true)).not.toBeNull()
  })
})

describe('the seed notice', () => {
  it('opens only after a seed and closes for good once acknowledged', () => {
    expect(satSeedNoticeOpen()).toBe(false)
    seedSatFavorites(
      view({ birds: [bird('SO-50', 27607)], passes: [pass('SO-50', 27607, 45)] }),
      true,
    )
    expect(satSeedNoticeOpen()).toBe(true)
    ackSatSeedNotice()
    expect(satSeedNoticeOpen()).toBe(false)
    expect(satSeedRecord()).not.toBeNull() // the marker outlives the notice
  })

  it('a corrupt marker reads as "never seeded" and never throws', () => {
    localStorage.setItem('nexus.sats.seeded', '{not json')
    expect(satSeedRecord()).toBeNull()
    expect(satSeedNoticeOpen()).toBe(false)
  })
})
