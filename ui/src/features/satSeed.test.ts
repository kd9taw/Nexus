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

// --- the stratified quota --------------------------------------------------
//
// THE OPERATOR'S CASE, verbatim: "FO-29 was not in favorites, but it's
// active." FO-29 is in the catalog, alive, amateur, with current elements —
// it simply lost the pass-count race. It flies 13.53 rev/day in an
// 800 × 1320 km ellipse; the birds that beat it fly ~15.3 rev/day at ~480 km,
// so they show more passes over any grid, every day, forever. Run the real
// predictor over the shipped catalog (six grids × four window starts,
// 2026-08-02 UTC) and FO-29 lands between 2nd and 81st of the ~332 birds with
// a workable pass, inside the geometry ten in 6 of the 24 runs: the SSB/CW
// transponders — the only birds this operator can work with an SSB rig and a
// hand-turned rotator — are not reliably last, they are reliably a coin toss.
//
// The bigger burial is what the ten is SPENT on: of the 367 birds in the
// shipped catalog that carry elements, 305 are beacon-only telemetry cubesats
// that cannot be worked at all, 18 of the top 20 by orbital rate are among
// them, and 3 to 9 of each pure-geometry ten was beacon-only. So these
// fixtures stack BOTH kinds of loser in front of FO-29 — the beacons that
// dominate the real pool and the FM birds the operator named.
describe('rankSatSeedCandidates — the ten span what the operator can WORK', () => {
  /** n passes at a plausible peak elevation, spread across the window. */
  const many = (name: string, norad: number, n: number, el = 40) =>
    Array.from({ length: n }, (_, i) => pass(name, norad, el, i))

  /** The real pool in miniature: beacon-only telemetry cubesats and FM birds
   * in low orbits, every one of them out-passing the one linear bird. */
  const buriedFo29 = () => {
    const birds: Bird[] = []
    const passes: ReturnType<typeof pass>[] = []
    // 12 beacon-only cubesats — the class that actually fills the top of the
    // pass-count table, and the class an operator cannot work at all.
    for (let i = 0; i < 12; i++) {
      const name = `CUBESAT-${String(i).padStart(2, '0')}`
      birds.push(bird(name, 90200 + i, { classes: ['beacon'] }))
      passes.push(...many(name, 90200 + i, 6, 55))
    }
    // 3 FM voice birds — workable, and still out-passing FO-29.
    for (const [name, norad] of [
      ['SO-50', 27607],
      ['AO-91', 43017],
      ['PO-101', 43678],
    ] as const) {
      birds.push(bird(name, norad, { classes: ['fm'] }))
      passes.push(...many(name, norad, 5, 50))
    }
    // 2 packet birds.
    for (const [name, norad] of [
      ['ISS', 25544],
      ['GREENCUBE', 53106],
    ] as const) {
      birds.push(bird(name, norad, { classes: ['digital'] }))
      passes.push(...many(name, norad, 4, 45))
    }
    // …and the bird the operator actually wanted: a high elliptical linear
    // transponder with the fewest passes in the set.
    birds.push(bird('FO-29', 24278, { classes: ['linear'] }))
    passes.push(...many('FO-29', 24278, 2, 35))
    return view({ birds, passes })
  }

  it("stars FO-29 even though every other bird out-passes it — the operator's case", () => {
    const picked = rankSatSeedCandidates(buriedFo29()).map((c) => c.name)
    expect(picked).toHaveLength(SAT_SEED_CAP)
    expect(picked).toContain('FO-29')
  })

  it('spends its ten on birds that can be worked before any beacon-only bird', () => {
    // The half of the operator's ruling that "guarantee one linear bird"
    // would have missed: beacon-only birds cannot be worked at ALL, so they
    // are the LAST call on a star, not the first.
    const classesOf = (name: string) =>
      buriedFo29().birds.find((b) => b.name === name)?.classes ?? []
    const picked = rankSatSeedCandidates(buriedFo29()).map((c) => c.name)
    const beacons = picked.filter((n) => classesOf(n).includes('beacon'))
    // 1 linear + 3 fm + 2 digital = 6 workable birds exist; the other four
    // slots fall to the beacons, by rank.
    expect(beacons).toHaveLength(4)
    expect(picked.filter((n) => !classesOf(n).includes('beacon')).sort()).toEqual(
      ['AO-91', 'FO-29', 'GREENCUBE', 'ISS', 'PO-101', 'SO-50'],
    )
  })

  it('rotates the classes, so no single class takes the whole set', () => {
    // Ten of each workable class available. The rotation gives 4 linear /
    // 3 fm / 3 digital — never ten FM birds because FM happens to fly lower.
    const birds: Bird[] = []
    const passes: ReturnType<typeof pass>[] = []
    const add = (cls: string, n: number, count: number, el: number) => {
      for (let i = 0; i < 10; i++) {
        const name = `${cls.toUpperCase()}-${String(i).padStart(2, '0')}`
        birds.push(bird(name, n + i, { classes: [cls] }))
        passes.push(...many(name, n + i, count, el))
      }
    }
    add('fm', 91000, 8, 60) // the low fliers sweep the geometry rank …
    add('digital', 92000, 6, 50)
    add('linear', 93000, 2, 30) // … and the linear birds are last, as in life
    const picked = rankSatSeedCandidates(view({ birds, passes })).map((c) => c.name)
    const count = (p: string) => picked.filter((n) => n.startsWith(p)).length
    expect(picked).toHaveLength(SAT_SEED_CAP)
    expect([count('LINEAR'), count('FM'), count('DIGITAL')]).toEqual([4, 3, 3])
  })

  it('still returns ten when the whole workable set is ONE class', () => {
    const birds: Bird[] = []
    const passes: ReturnType<typeof pass>[] = []
    for (let i = 0; i < 14; i++) {
      const name = `FMBIRD-${String(i).padStart(2, '0')}`
      birds.push(bird(name, 94000 + i, { classes: ['fm'] }))
      passes.push(...many(name, 94000 + i, 14 - i, 40))
    }
    const picked = rankSatSeedCandidates(view({ birds, passes }))
    expect(picked).toHaveLength(SAT_SEED_CAP)
    // …and the geometry rank still orders them INSIDE that one class.
    expect(picked.map((c) => c.name)).toEqual(
      Array.from({ length: 10 }, (_, i) => `FMBIRD-${String(i).padStart(2, '0')}`),
    )
  })

  it('an UNCLASSIFIED catalog seeds exactly as it always did', () => {
    // Not theoretical: the installer's bundled seed snapshot carries no
    // classification until it is re-cut, the Celestrak fallback leg never
    // carries one, and a build that predates this field rewrites the on-disk
    // catalog WITHOUT it. All three must land on today's top-ten-by-passes.
    const birds: Bird[] = []
    const passes: ReturnType<typeof pass>[] = []
    for (let i = 0; i < 14; i++) {
      const name = `PLAIN-${String(i).padStart(2, '0')}`
      birds.push(bird(name, 95000 + i)) // no `classes` key at all
      passes.push(...many(name, 95000 + i, 14 - i, 40))
    }
    const v = view({ birds, passes })
    const picked = rankSatSeedCandidates(v).map((c) => c.name)
    expect(picked).toEqual(
      // the pre-classification answer: most passes first, ties on elevation
      [...v.birds]
        .map((b) => b.name)
        .sort()
        .slice(0, SAT_SEED_CAP),
    )
  })

  it('ranks an empty class list and an absent one alike — both behind a workable bird', () => {
    // The two states the WIRE keeps apart (`[]` = classified with nothing left
    // to work; absent = never classified) reach the same answer HERE, and that
    // is the design, not an oversight: neither offers a class the rotation can
    // place, so both fall to the residual in geometry rank. The test names it
    // so nobody "fixes" one of them into jumping the queue.
    //
    // Both cases are run, side by side, precisely because a single case with
    // one expectation would pass whether or not the code distinguished them.
    const contender = (classes: string[] | undefined) =>
      view({
        birds: [
          bird('LOUD-BUT-UNWORKABLE', 90500, classes === undefined ? {} : { classes }),
          bird('RS-44', 44909, { classes: ['linear'] }),
        ],
        passes: [...many('LOUD-BUT-UNWORKABLE', 90500, 9, 70), ...many('RS-44', 44909, 1, 20)],
      })
    // One slot, and it goes to the workable bird in BOTH readings — even
    // though the other bird has nine times the passes and twice the elevation.
    expect(rankSatSeedCandidates(contender([]), 1).map((c) => c.name)).toEqual(['RS-44'])
    expect(rankSatSeedCandidates(contender(undefined), 1).map((c) => c.name)).toEqual(['RS-44'])
    // …and with room for both, the residual takes the loud one on geometry —
    // again identically, and the returned order is rank order, not pick order.
    for (const classes of [[], undefined] as (string[] | undefined)[]) {
      expect(rankSatSeedCandidates(contender(classes), 2).map((c) => c.name)).toEqual([
        'LOUD-BUT-UNWORKABLE',
        'RS-44',
      ])
    }
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
