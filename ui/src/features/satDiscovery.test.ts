// The discovery band's pure layer: the per-bird rollup, the worth order, the
// admission rules. Every case here is a measured live defect or a stated law
// from the design ruling — not a toy.
import { describe, it, expect } from 'vitest'
import type { SatPass, SatView } from '../types'
import {
  DISCOVERY_ROW_CAP,
  modePillWord,
  rollPassesToBirds,
} from './satDiscovery'

const NOW = 1_754_000_000

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

const pass = (name: string, norad: number | null, maxElDeg: number, aosOffsetMin = 30): SatPass => ({
  name,
  norad,
  aosUnix: NOW + aosOffsetMin * 60,
  losUnix: NOW + aosOffsetMin * 60 + 600,
  maxElDeg,
  aosAzDeg: 0,
  losAzDeg: 180,
})

const view = (birds: Bird[], passes: SatPass[]): SatView => ({
  tleAgeDays: 1,
  usableCount: 300,
  agingCount: 0,
  heldBackCount: 0,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds,
  passes,
  excluded: [],
})

const noFav = () => false

describe('rollPassesToBirds — the volume law', () => {
  it('rolls many passes into ONE row per bird, keyed on NORAD', () => {
    const v = view(
      [bird('RS-44', 44909)],
      [pass('RS-44', 44909, 40, 30), pass('RS-44', 44909, 70, 200), pass('RS-44', 44909, 20, 400)],
    )
    const rows = rollPassesToBirds(v, NOW, noFav)
    expect(rows.length).toBe(1)
    expect(rows[0].workable).toBe(3)
    // The row shows the bird's BEST pass, not its soonest.
    expect(rows[0].best.maxElDeg).toBe(70)
  })

  it('a 16-pass rideshare clump does NOT outrank a 70° linear bird (the measured seed distortion)', () => {
    const clumpPasses = Array.from({ length: 16 }, (_, i) => pass('CLUMP-SAT', 99001, 40, 30 + i * 80))
    const v = view(
      [bird('CLUMP-SAT', 99001), bird('FO-29', 24278)],
      [...clumpPasses, pass('FO-29', 24278, 70, 300)],
    )
    const rows = rollPassesToBirds(v, NOW, noFav)
    expect(rows[0].name).toBe('FO-29')
  })

  it('a bird below the workability floor never appears, at any disclosure level', () => {
    const v = view([bird('GRAZER-1', 99002)], [pass('GRAZER-1', 99002, 8), pass('GRAZER-1', 99002, 9)])
    expect(rollPassesToBirds(v, NOW, noFav).length).toBe(0)
  })

  it('a positively dead bird sinks out entirely; an UNKNOWN status is admitted, never dropped', () => {
    // MYSTERY-1 carries the REAL catalog-absent wire shape: `status` absent
    // and `amateur: false` — catalog_marks returns (None, false) for every
    // bird it was never asked about, and the bool rides the wire
    // unconditionally. (An earlier draft used `amateur: undefined`, a shape
    // the backend never produces for a placeable bird — it masked the gate
    // reading the unconditional false as a real answer.)
    const v = view(
      [bird('DEADBIRD', 99003, { status: 'dead' }), bird('MYSTERY-1', 99004, { status: undefined, amateur: false })],
      [pass('DEADBIRD', 99003, 60), pass('MYSTERY-1', 99004, 40)],
    )
    const rows = rollPassesToBirds(v, NOW, noFav)
    expect(rows.map((r) => r.name)).toEqual(['MYSTERY-1'])
  })

  it('amateur:false is believed ONLY beside a present status (the satHealth rule, on the wire shape the backend actually emits)', () => {
    // The wire contract (SatBird.amateur's own doc): the bool is serialized
    // unconditionally and is false-with-absent-status for every bird the
    // catalog was never ASKED about — operator-imported TLEs, Celestrak-leg
    // birds, catalog-less snapshots. Absence reads as NOT ASKED, never as
    // "no transmitters".
    const v = view(
      [
        // Never asked: status absent + amateur:false → ADMITTED.
        bird('IMPORTED-1', 99020, { status: undefined, amateur: false }),
        // Asked and answered: status present + amateur:false → excluded.
        bird('QUIET-1', 99021, { status: 'alive', amateur: false }),
      ],
      [pass('IMPORTED-1', 99020, 40), pass('QUIET-1', 99021, 60)],
    )
    expect(rollPassesToBirds(v, NOW, noFav).map((r) => r.name)).toEqual(['IMPORTED-1'])
  })

  it('★ birds are excluded — the schedule above owns them', () => {
    const v = view([bird('RS-44', 44909)], [pass('RS-44', 44909, 70)])
    expect(rollPassesToBirds(v, NOW, (n) => n === 'RS-44').length).toBe(0)
  })

  it('expired passes neither count nor lead', () => {
    const gone: SatPass = { ...pass('RS-44', 44909, 80), aosUnix: NOW - 1200, losUnix: NOW - 600 }
    const v = view([bird('RS-44', 44909)], [gone, pass('RS-44', 44909, 30)])
    const rows = rollPassesToBirds(v, NOW, noFav)
    expect(rows[0].workable).toBe(1)
    expect(rows[0].best.maxElDeg).toBe(30)
  })

  it('placeholder OBJECT names and duplicate names are excluded (the star collision is unrepresentable)', () => {
    const v = view(
      [bird('OBJECT D', 99005), bird('TWIN', 99006), bird('TWIN', 99007)],
      [pass('OBJECT D', 99005, 70), pass('TWIN', 99006, 60), pass('TWIN', 99007, 50)],
    )
    expect(rollPassesToBirds(v, NOW, noFav).length).toBe(0)
  })

  it('the list is worth-sorted, so the row cap can never hide the best bird', () => {
    const birds = Array.from({ length: 20 }, (_, i) => bird(`SAT-${String(i).padStart(2, '0')}`, 90000 + i))
    const passes = birds.map((b, i) => pass(b.name, b.norad ?? null, 15 + i * 3, 30 + i))
    const rows = rollPassesToBirds(view(birds, passes), NOW, noFav)
    expect(rows.length).toBe(20)
    // Descending worth: the top of the capped slice IS the best bird.
    expect(rows[0].name).toBe('SAT-19')
    const capped = rows.slice(0, DISCOVERY_ROW_CAP)
    expect(capped[0].best.maxElDeg).toBe(Math.max(...rows.map((r) => r.best.maxElDeg)))
  })

  it('mode class is a TIER: linear outranks FM outranks unknown outranks digital/beacon', () => {
    const v = view(
      [
        bird('DIGI-1', 99010, { classes: ['digital'] }),
        bird('NOCLASS-1', 99011),
        bird('AO-91', 43017, { classes: ['fm'] }),
        bird('RS-44', 44909, { classes: ['linear'] }),
        bird('BCN-1', 99012, { classes: ['beacon'] }),
      ],
      [
        // The linear bird flies the WORST geometry — the tier still wins.
        pass('DIGI-1', 99010, 80),
        pass('NOCLASS-1', 99011, 70),
        pass('AO-91', 43017, 60),
        pass('RS-44', 44909, 25),
        pass('BCN-1', 99012, 85),
      ],
    )
    expect(rollPassesToBirds(v, NOW, noFav).map((r) => r.name)).toEqual([
      'RS-44',
      'AO-91',
      'NOCLASS-1',
      'DIGI-1',
      'BCN-1',
    ])
  })

  it('carries the live altitude from the position snapshot when the bird has one', () => {
    const v = view([bird('RS-44', 44909, { altKm: 1522 })], [pass('RS-44', 44909, 70)])
    expect(rollPassesToBirds(v, NOW, noFav)[0].altKm).toBe(1522)
  })
})

describe('modePillWord — kindWord’s law for the class field', () => {
  it('maps the four canonical classes to operator words', () => {
    expect(modePillWord('fm')).toBe('FM voice')
    expect(modePillWord('linear')).toBe('Linear SSB/CW')
    expect(modePillWord('digital')).toBe('Digital')
    expect(modePillWord('beacon')).toBe('Beacon')
  })
  it('absent or unknown renders NO pill — never a guess', () => {
    expect(modePillWord(null)).toBeNull()
    expect(modePillWord(undefined)).toBeNull()
    expect(modePillWord('')).toBeNull()
    expect(modePillWord('transponder')).toBeNull()
  })
})
