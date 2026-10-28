import { describe, expect, it } from 'vitest'
import type { AprsStation } from '../api'
import {
  APRS_MAP_DEFAULTS,
  aprsLabelOrder,
  aprsMapPrefsValue,
  aprsTrail,
  filterAprsStations,
  parseAprsMapPrefs,
  placeAprsLabels,
  trackSummary,
} from './aprsMap'
import { makeProjection, viewCentredOn } from '../mapGeo'

const NOW = 100_000

function st(call: string, over: Partial<AprsStation> = {}): AprsStation {
  return {
    call,
    lat: 39,
    lon: -97,
    symbolTable: '/',
    symbolCode: '-',
    kind: 'position',
    text: '',
    speedKnots: null,
    courseDeg: null,
    path: [],
    raw: '',
    lastHeardUnix: NOW - 60,
    lastRfUnix: NOW - 60,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 1,
    firstHeardUnix: NOW - 600,
    wx: null,
    ...over,
  }
}

describe('filterAprsStations', () => {
  const house = st('W9FIX') // '/-' = house: fixed
  const car = st('W9CAR-9', { symbolCode: '>' }) // car: mobile
  const wx = st('W9WX', { symbolCode: '_' }) // weather
  const old = st('W9OLD', { lastHeardUnix: NOW - 45 * 60 })
  const inet = st('W9NET', { sourceKind: 'inet', lastRfUnix: null, lastInetUnix: NOW - 30 })
  const all = [house, car, wx, old, inet]

  it('passes everything through with the defaults', () => {
    expect(filterAprsStations(all, { showInet: true, ageMin: 0, type: 'all' }, NOW).map((s) => s.call)).toEqual(
      all.map((s) => s.call),
    )
  })

  it('keeps one kind of station by its symbol family', () => {
    expect(filterAprsStations(all, { showInet: true, ageMin: 0, type: 'mobile' }, NOW).map((s) => s.call)).toEqual([
      'W9CAR-9',
    ])
    expect(filterAprsStations(all, { showInet: true, ageMin: 0, type: 'wx' }, NOW).map((s) => s.call)).toEqual(['W9WX'])
  })

  it('drops stations last heard before the age window', () => {
    const got = filterAprsStations(all, { showInet: true, ageMin: 30, type: 'all' }, NOW).map((s) => s.call)
    expect(got).not.toContain('W9OLD')
    expect(got).toContain('W9FIX')
  })

  it('still honours the internet toggle, combined with the others', () => {
    const got = filterAprsStations(all, { showInet: false, ageMin: 30, type: 'fixed' }, NOW).map((s) => s.call)
    expect(got).toEqual(['W9FIX'])
  })
})

describe('aprsTrail', () => {
  const track = [
    { lat: 39.0, lon: -97.0, atUnix: NOW - 50 * 60 },
    { lat: 39.01, lon: -97.0, atUnix: NOW - 20 * 60 },
    { lat: 39.02, lon: -97.0, atUnix: NOW - 60 },
  ]
  it('is the track, oldest first', () => {
    expect(aprsTrail(st('W9CAR-9', { track }), NOW, 0)).toEqual(track)
  })
  it('is held to the age filter', () => {
    expect(aprsTrail(st('W9CAR-9', { track }), NOW, 30)).toEqual(track.slice(1))
  })
  it('is empty when fewer than two points are left: one point is not a trail', () => {
    expect(aprsTrail(st('W9CAR-9', { track }), NOW, 10)).toEqual([])
    expect(aprsTrail(st('W9FIX'), NOW, 0)).toEqual([])
  })
})

describe('trackSummary', () => {
  it('is the distance along the track and the time it spans', () => {
    const s = trackSummary([
      { lat: 0, lon: 0, atUnix: 1000 },
      { lat: 0, lon: 0.1, atUnix: 1300 },
      { lat: 0.1, lon: 0.1, atUnix: 1600 },
    ])
    expect(s).not.toBeNull()
    expect(s!.km).toBeCloseTo(22.24, 1)
    expect(s!.secs).toBe(600)
  })
  it('is null without a track', () => {
    expect(trackSummary(undefined)).toBeNull()
    expect(trackSummary([{ lat: 0, lon: 0, atUnix: 1 }])).toBeNull()
  })
})

describe('aprsLabelOrder', () => {
  const a = st('A', { lastHeardUnix: NOW - 10 })
  const b = st('B', { lastHeardUnix: NOW - 5, speedKnots: 30, courseDeg: 90 })
  const c = st('C', { lastHeardUnix: NOW - 1 })
  const sel = st('S', { lastHeardUnix: NOW - 900 })
  it('puts the selection first, then moving stations, then the most recently heard', () => {
    expect(aprsLabelOrder([a, b, c, sel], 'S', true)).toEqual(['S', 'B', 'C', 'A'])
  })
  it('with labels off, names only the selection and the moving stations', () => {
    expect(aprsLabelOrder([a, b, c, sel], 'S', false)).toEqual(['S', 'B'])
  })
})

describe('placeAprsLabels', () => {
  const box = { w: 400, h: 300 }
  it('puts a lone label to the right of its symbol', () => {
    const got = placeAprsLabels([{ key: 'A', x: 100, y: 100, w: 40, h: 12, clear: 10 }], box, [])
    expect(got).toEqual([{ key: 'A', x: 110, y: 100 }])
  })
  it('never lets two labels overlap: the second tries another side, and is dropped when none is free', () => {
    const cands = [
      { key: 'A', x: 100, y: 100, w: 40, h: 12, clear: 10 },
      { key: 'B', x: 102, y: 102, w: 40, h: 12, clear: 10 },
      { key: 'C', x: 101, y: 101, w: 40, h: 12, clear: 10 },
      { key: 'D', x: 103, y: 99, w: 40, h: 12, clear: 10 },
      { key: 'E', x: 100, y: 100, w: 40, h: 12, clear: 10 },
    ]
    const got = placeAprsLabels(cands, box, [])
    const rects = got.map((g) => ({ x0: g.x, x1: g.x + 40, y0: g.y - 6, y1: g.y + 6 }))
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const r = rects[i]
        const q = rects[j]
        expect(r.x0 < q.x1 && q.x0 < r.x1 && r.y0 < q.y1 && q.y0 < r.y1).toBe(false)
      }
    }
    expect(got[0].key).toBe('A')
    expect(got.length).toBeLessThan(cands.length)
  })
  it('keeps a label off another station’s symbol', () => {
    const got = placeAprsLabels(
      [{ key: 'A', x: 100, y: 100, w: 40, h: 12, clear: 10 }],
      box,
      [{ x: 130, y: 100, r: 8 }],
    )
    expect(got).toHaveLength(1)
    expect(got[0].x).toBeLessThan(100) // flipped to the left
  })
  it('keeps a label inside the map', () => {
    const got = placeAprsLabels([{ key: 'A', x: 390, y: 100, w: 40, h: 12, clear: 10 }], box, [])
    expect(got[0].x + 40).toBeLessThanOrEqual(400)
  })
})

describe('APRS map prefs', () => {
  it('reads nothing as the defaults', () => {
    expect(parseAprsMapPrefs(null)).toEqual(APRS_MAP_DEFAULTS)
    expect(parseAprsMapPrefs('not json')).toEqual(APRS_MAP_DEFAULTS)
  })
  it('round-trips a full record', () => {
    const p = { pick: 'street' as const, ageMin: 30 as const, type: 'mobile' as const, tracks: false, labels: false }
    expect(parseAprsMapPrefs(aprsMapPrefsValue(p))).toEqual(p)
  })
  it('replaces each unknown field with its default, keeping the rest', () => {
    const got = parseAprsMapPrefs(JSON.stringify({ pick: '3d', ageMin: 7, type: 'spaceship', tracks: false }))
    expect(got).toEqual({ ...APRS_MAP_DEFAULTS, tracks: false })
  })
})

describe('viewCentredOn', () => {
  const me = { lat: 39, lon: -97 }
  const there = { lat: 40, lon: -95 }
  const base = { zoom: 25, rotate: null, panX: 0, panY: 0 }
  for (const kind of ['globe', 'world', 'aeqd'] as const) {
    it(`puts the focus in the middle of the ${kind} map`, () => {
      const v = viewCentredOn(kind, me, there, 800, 600, base)
      const p = makeProjection(kind, me, 800, 600, v)([there.lon, there.lat])!
      expect(p[0]).toBeCloseTo(400, 3)
      expect(p[1]).toBeCloseTo(300, 3)
    })
  }
  it('keeps the operator’s own pan on top of the centring', () => {
    const v = viewCentredOn('world', me, there, 800, 600, { ...base, panX: 30, panY: -20 })
    const p = makeProjection('world', me, 800, 600, v)([there.lon, there.lat])!
    expect(p[0]).toBeCloseTo(430, 3)
    expect(p[1]).toBeCloseTo(280, 3)
  })
  it('leaves a globe the operator has spun alone', () => {
    const spun = { ...base, rotate: [10, 20] as [number, number] }
    expect(viewCentredOn('globe', me, there, 800, 600, spun)).toEqual(spun)
  })
})
