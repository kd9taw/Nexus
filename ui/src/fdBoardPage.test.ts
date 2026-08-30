// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { ARRL_SECTIONS_BY_DIVISION } from './features/arrlSections'
import { BAND_COLOR, bandColor } from './bandColors'

// The Field Day spectator scoreboard page, executed for real: the page keeps
// all rendering in pure render(data, meta) functions behind the __fdboard test
// seam (no network, no timers under __FDBOARD_TEST__), so jsdom can drive the
// exact script the TV runs. Server-side truth (scoring math, dedupe, payload
// shape) is pinned in Rust — crates/tempo-app/src/fd_scoreboard.rs; this file
// covers what the PAGE does with a payload: the globe that IS the section
// board, ticker attribution, the WFD no-power-math rendering, and the stale
// badge. The globe's geometry and its arc bookkeeping are pure functions
// behind __fdboard.globe, so they are checked exactly here; the PAINTING is
// not — jsdom's canvas.getContext('2d') returns null, which is itself the
// case the page must survive (a TV browser with canvas disabled still gets a
// working scoreboard).
// (jsdom never lays out — visual/layout checks are the manual puppeteer pass.)

const html = readFileSync(
  resolve(process.cwd(), '../crates/tempo-app/assets/fd_scoreboard.html'),
  'utf8',
)
const body = /<body>([\s\S]*)<\/body>/.exec(html)?.[1]
const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]

type LatLon = [number, number]
type Globe = {
  SECTION_LATLON: Record<string, LatLon>
  sectionLatLon: (code: unknown) => LatLon | null
  bandColor: (band: string) => string
  project: (lat: number, lon: number, rotDeg: number) => { x: number; y: number; z: number }
  arcPoints: (a: LatLon, b: LatLon, n: number) => LatLon[]
  subsolarPoint: (ms: number) => { lat: number; lon: number }
  terminatorRing: (ms: number, n: number) => LatLon[]
  contactKey: (t: { when_unix: number; call: string; band: string; mode: string }) => string
  landRingCount: () => number
  snapshot: () => {
    lit: string[]
    home: LatLon | null
    arcs: number
    fired: number
    seeded: boolean
  }
}

type Board = {
  STRINGS: Record<string, string>
  render: (data: unknown, meta: unknown) => void
  renderMeta: (meta: unknown) => void
  setStale: (on: boolean) => void
  setInactive: (on: boolean) => void
  tickClock: () => void
  globe: Globe
}

/** Angular distance between two lat/lon points, degrees — the terminator check. */
function angularDeg(a: LatLon, b: LatLon): number {
  const D = Math.PI / 180
  const c =
    Math.sin(a[0] * D) * Math.sin(b[0] * D) +
    Math.cos(a[0] * D) * Math.cos(b[0] * D) * Math.cos((a[1] - b[1]) * D)
  return Math.acos(Math.max(-1, Math.min(1, c))) / D
}

/** Every code in the canonical 83-section universe (the globe's pin table is
 *  checked against THIS, not against itself). */
const ALL_SECTION_CODES = ARRL_SECTIONS_BY_DIVISION.flatMap((d) =>
  d.sections.map((sec) => sec.code),
)

declare global {
  interface Window {
    __FDBOARD_TEST__?: boolean
    __fdboard?: Board
  }
}

function boot(): Board {
  window.__FDBOARD_TEST__ = true
  document.body.innerHTML = body!
  new Function(script!)()
  return window.__fdboard!
}

// A meta fixture shaped like /scoreboard/meta.json: a full 83-code section
// universe (the page must build one chip per code, grouped by division).
const DIVISIONS = ['Atlantic', 'Central', 'New England', 'Pacific', 'RAC']
function makeMeta(scoringModel: 'powered' | 'objectives') {
  const sections = []
  for (let i = 0; i < 83; i++) {
    sections.push({
      code: `S${String(i).padStart(2, '0')}`,
      name: `Section ${i}`,
      division: DIVISIONS[i % DIVISIONS.length],
    })
  }
  return {
    event: {
      kind: scoringModel === 'objectives' ? 'wfd' : 'arrlfd',
      name: scoringModel === 'objectives' ? 'Winter Field Day' : 'ARRL Field Day',
      year: 2026,
      start_unix: 1_750_000_000,
      end_unix: 1_750_097_200,
      call: 'W9ABC',
      class: '3A',
      section: 'WI',
    },
    scoring_model: scoringModel,
    rules_year: 2026,
    rules_generated: '2026-08-29T00:00:00Z',
    sections,
    bonuses: [
      { id: 'emergency-power', label: 'Emergency power', points: 100 },
      { id: 'safety-officer', label: 'Safety officer', points: 100 },
      { id: 'web-submission', label: 'Web submission', points: 50 },
    ],
  }
}

function makeData(model: 'powered' | 'objectives') {
  const meta = makeMeta(model)
  const score =
    model === 'powered'
      ? {
          model: 'powered',
          qso_points: 6,
          bonus_points: 150,
          power_mult: 2,
          powered_points: 12,
          total: 162,
        }
      : {
          model: 'objectives',
          qso_points: 6,
          bonus_points: 150,
          total: 6,
          projected_at_submission: 18,
          objectives_claimed: 2,
        }
  return {
    rev: 7,
    now_unix: 1_750_007_500,
    event: meta.event,
    score,
    qsos: { count: 4, rate_hour: 12, rate_10min: 3, hourly: [2, 2, 0] },
    ticker: [
      {
        when_unix: 1_750_007_000,
        call: 'VE3AAA',
        class: '3A',
        section: 'S05',
        band: '20m',
        mode: 'DIG',
        submode: 'FT8',
        position: 'CW tent',
        operator: 'W9AAA',
      },
      {
        when_unix: 1_750_004_000,
        call: 'K1ABC',
        class: '2A',
        section: 'S01',
        band: '20m',
        mode: 'PH',
        submode: '',
        position: 'Phone tent',
        operator: 'W9BBB',
      },
    ],
    band_mode: [
      { band: '40m', ph: 1, cw: 0, dig: 0 },
      { band: '20m', ph: 1, cw: 1, dig: 1 },
    ],
    sections_worked: ['S01', 'S05'],
    positions: [
      {
        id: 'aaaa1111',
        label: 'CW tent',
        operator: 'W9AAA',
        qsos_raw: 2,
        qsos_unique: 2,
        points: 4,
        last_qso_unix: 1_750_007_000,
      },
      {
        id: 'bbbb2222',
        label: 'Phone tent',
        operator: 'W9BBB',
        qsos_raw: 3,
        qsos_unique: 2,
        points: 2,
        last_qso_unix: 1_750_004_000,
      },
    ],
    claimed: ['emergency-power', 'web-submission'],
  }
}

// The synthetic S00…S82 codes above are deliberately NOT real sections — they
// prove the globe treats an unknown code as a no-op. These two build a payload
// out of REAL codes, which is what actually lights pins and fires arcs.
function realMeta() {
  const m = makeMeta('powered')
  m.event.section = 'WI'
  return m
}
function realData() {
  const d = makeData('powered')
  d.event.section = 'WI'
  d.sections_worked = ['EMA', 'ONE']
  d.ticker = [
    {
      when_unix: 1_750_007_000,
      call: 'VE3AAA',
      class: '3A',
      section: 'ONE',
      band: '20m',
      mode: 'DIG',
      submode: 'FT8',
      position: 'CW tent',
      operator: 'W9AAA',
    },
    {
      when_unix: 1_750_004_000,
      call: 'K1ABC',
      class: '2A',
      section: 'EMA',
      band: '20m',
      mode: 'PH',
      submode: '',
      position: 'Phone tent',
      operator: 'W9BBB',
    },
  ]
  return d
}

beforeEach(() => {
  // A fresh DOM + a fresh script run per test (the script keeps module state:
  // metaDone, lastHeroKey).
  document.body.innerHTML = ''
})

describe('fd scoreboard page', () => {
  it('exposes the test seam with pure render functions and the locale seam', () => {
    const b = boot()
    expect(typeof b.render).toBe('function')
    expect(typeof b.renderMeta).toBe('function')
    // Every page string lives in ONE object — the later-locale injection seam.
    expect(Object.keys(b.STRINGS).length).toBeGreaterThanOrEqual(30)
  })

  it('has a pin for every one of the 83 sections in the rules universe', () => {
    const b = boot()
    expect(ALL_SECTION_CODES.length).toBe(83)
    const missing = ALL_SECTION_CODES.filter((c) => b.globe.sectionLatLon(c) === null)
    expect(missing).toEqual([])
    // POSITIVE CONTROL: the same lookup DOES come back null for a non-section,
    // so the empty list above is the table being complete, not the check being
    // broken.
    expect(b.globe.sectionLatLon('ZZZ')).toBeNull()
    // Every pin is a real place, not a 0,0 placeholder off West Africa, and
    // every one is on the globe.
    for (const code of ALL_SECTION_CODES) {
      const [lat, lon] = b.globe.sectionLatLon(code)!
      expect(Math.abs(lat), code).toBeGreaterThan(1)
      expect(Math.abs(lon), code).toBeGreaterThan(1)
      expect(Math.abs(lat), code).toBeLessThan(90)
      expect(Math.abs(lon), code).toBeLessThanOrEqual(180)
    }
    // The table carries the universe and nothing else.
    expect(Object.keys(b.globe.SECTION_LATLON).sort()).toEqual(
      [...ALL_SECTION_CODES].sort(),
    )
  })

  it('lights exactly the worked sections and shows the n-of-83 count', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    // The lit set is the PAYLOAD's, not the pin table's — a code with no pin
    // still counts on the header, it just has nowhere to be drawn.
    expect(b.globe.snapshot().lit).toEqual(['S01', 'S05'])
    expect(document.getElementById('sec-count')!.textContent).toContain('2')
    expect(document.getElementById('sec-count')!.textContent).toContain('83')
    // Real codes light real pins.
    const b2 = boot()
    b2.render(realData(), realMeta())
    expect(b2.globe.snapshot().lit).toEqual(['EMA', 'ONE'])
    expect(b2.globe.snapshot().home).toEqual(b2.globe.sectionLatLon('WI'))
  })

  it('renders the globe canvas and survives a browser with no 2D context', () => {
    const b = boot()
    b.renderMeta(makeMeta('powered'))
    expect(document.getElementById('globe')).not.toBeNull()
    // jsdom's getContext('2d') returns null; nothing above may throw on it.
    expect(
      (document.getElementById('globe') as HTMLCanvasElement).getContext('2d'),
    ).toBeNull()
    const legend = document.getElementById('globe-legend')!.textContent!
    expect(legend).toContain(b.STRINGS.legendWorked)
    expect(legend).toContain(b.STRINGS.legendUnworked)
    expect(() => b.render(realData(), realMeta())).not.toThrow()
    // The chip board is gone — the globe replaced it, it did not join it.
    expect(document.getElementById('sec-board')).toBeNull()
  })

  it('skips an unknown, empty or DX section rather than pinning it at 0,0', () => {
    const b = boot()
    for (const bad of ['', 'DX', 'S00', 'zz', 'constructor', 'toString', 'hasOwnProperty']) {
      expect(b.globe.sectionLatLon(bad), bad).toBeNull()
    }
    expect(b.globe.sectionLatLon(null)).toBeNull()
    expect(b.globe.sectionLatLon(undefined)).toBeNull()
    // A DX contact carries no section, so it gets no arc — it is still on the
    // ticker and in the score, it is simply not a place this page can draw.
    const meta = realMeta()
    b.render(realData(), meta)
    const dx = realData()
    dx.ticker.unshift({
      when_unix: 1_750_008_000,
      call: 'DL1ABC',
      class: '1D',
      section: 'DX',
      band: '20m',
      mode: 'CW',
      submode: '',
      position: 'CW tent',
      operator: 'W9AAA',
    })
    b.render(dx, meta)
    expect(b.globe.snapshot().fired).toBe(0)
  })

  it('fires no arc when the club station has no section of its own', () => {
    const b = boot()
    const meta = realMeta()
    const d1 = realData()
    d1.event.section = ''
    b.render(d1, meta)
    const d2 = realData()
    d2.event.section = ''
    d2.ticker.unshift({
      when_unix: 1_750_008_000,
      call: 'W1AW',
      class: '2A',
      section: 'EMA',
      band: '40m',
      mode: 'CW',
      submode: '',
      position: 'CW tent',
      operator: 'W9AAA',
    })
    b.render(d2, meta)
    expect(b.globe.snapshot().home).toBeNull()
    expect(b.globe.snapshot().fired).toBe(0)
  })

  it('seeds the first payload silently, then fires one arc per NEW contact only', () => {
    const b = boot()
    const meta = realMeta()
    // A TV plugged in at hour 18 must not launch 25 arcs at once.
    b.render(realData(), meta)
    expect(b.globe.snapshot().seeded).toBe(true)
    expect(b.globe.snapshot().fired).toBe(0)

    // The rev counter suppresses an identical payload, but a payload that
    // changed for some OTHER reason re-delivers the same ticker rows. Those
    // are not new contacts.
    b.render(realData(), meta)
    expect(b.globe.snapshot().fired).toBe(0)

    const d2 = realData()
    d2.ticker.unshift({
      when_unix: 1_750_008_000,
      call: 'W1AW',
      class: '2A',
      section: 'EMA',
      band: '40m',
      mode: 'CW',
      submode: '',
      position: 'CW tent',
      operator: 'W9AAA',
    })
    b.render(d2, meta)
    expect(b.globe.snapshot().fired).toBe(1)
    expect(b.globe.snapshot().arcs).toBe(1)

    // Poll again with that same row present: still one arc, ever.
    b.render(d2, meta)
    b.render(d2, meta)
    expect(b.globe.snapshot().fired).toBe(1)
    expect(b.globe.snapshot().arcs).toBe(1)
  })

  it('queues no arcs at all when the viewer asked for reduced motion', () => {
    // jsdom defines no matchMedia at all, which is why the page guards on
    // typeof — and why every OTHER test in this file is the positive control
    // for this one: with matchMedia absent, the same sequence fires an arc.
    const w = window as unknown as { matchMedia?: (q: string) => unknown }
    w.matchMedia = (q: string) => ({ matches: /reduce/.test(q) })
    try {
      const b = boot()
      const meta = realMeta()
      b.render(realData(), meta)
      const d2 = realData()
      d2.ticker.unshift({
        when_unix: 1_750_008_000,
        call: 'W1AW',
        class: '2A',
        section: 'EMA',
        band: '40m',
        mode: 'CW',
        submode: '',
        position: 'CW tent',
        operator: 'W9AAA',
      })
      b.render(d2, meta)
      expect(b.globe.snapshot().fired).toBe(0)
      expect(b.globe.snapshot().arcs).toBe(0)
      // The board is still a board: the pins are lit and the count is right.
      expect(b.globe.snapshot().lit).toEqual(['EMA', 'ONE'])
      expect(document.getElementById('sec-count')!.textContent).toContain('83')
    } finally {
      delete w.matchMedia
    }
  })

  it('caps arcs in flight so a burst cannot pile up', () => {
    const b = boot()
    const meta = realMeta()
    b.render(realData(), meta)
    const burst = realData()
    for (let i = 0; i < 30; i++) {
      burst.ticker.unshift({
        when_unix: 1_750_008_000 + i,
        call: `N${i}XX`,
        class: '2A',
        section: 'EMA',
        band: '40m',
        mode: 'CW',
        submode: '',
        position: 'CW tent',
        operator: 'W9AAA',
      })
    }
    b.render(burst, meta)
    expect(b.globe.snapshot().fired).toBe(30)
    expect(b.globe.snapshot().arcs).toBeLessThanOrEqual(12)
  })

  it('keys a contact by time, call, band and mode', () => {
    const b = boot()
    const base = { when_unix: 100, call: 'W1AW', band: '20m', mode: 'CW' }
    expect(b.globe.contactKey(base)).toBe(b.globe.contactKey({ ...base }))
    expect(b.globe.contactKey(base)).not.toBe(
      b.globe.contactKey({ ...base, mode: 'PH' }),
    )
    expect(b.globe.contactKey(base)).not.toBe(
      b.globe.contactKey({ ...base, when_unix: 101 }),
    )
  })

  it('projects onto the unit sphere and culls the far side', () => {
    const b = boot()
    const front = b.globe.project(0, 0, 0)
    expect(front.z).toBeGreaterThan(0.9)
    expect(front.x).toBeCloseTo(0, 12)
    const back = b.globe.project(0, 180, 0)
    expect(back.z).toBeLessThan(-0.9)
    // Rotation is what turns the globe: 90° east brings lon −90 to the centre.
    expect(b.globe.project(0, -90, 90).z).toBeGreaterThan(0.9)
    // Nothing leaves the unit sphere, so the screen radius is always the
    // globe radius.
    for (const [lat, lon] of [
      [0, 0],
      [45, -90],
      [-33, 170],
      [89, 12],
    ]) {
      const p = b.globe.project(lat, lon, 37)
      expect(Math.sqrt(p.x * p.x + p.y * p.y + p.z * p.z)).toBeCloseTo(1, 9)
    }
  })

  it('interpolates a great circle that starts and ends on its endpoints', () => {
    const b = boot()
    const home = b.globe.sectionLatLon('WI')!
    const dx = b.globe.sectionLatLon('EMA')!
    const pts = b.globe.arcPoints(home, dx, 8)
    expect(pts.length).toBe(9)
    expect(pts[0][0]).toBeCloseTo(home[0], 6)
    expect(pts[0][1]).toBeCloseTo(home[1], 6)
    expect(pts[8][0]).toBeCloseTo(dx[0], 6)
    expect(pts[8][1]).toBeCloseTo(dx[1], 6)
    // The path stays between its endpoints in longitude and does not dive to
    // the equator (a straight lat/lon lerp would be a different, wrong line).
    for (const [lat, lon] of pts) {
      expect(lon).toBeGreaterThanOrEqual(home[1] - 1e-6)
      expect(lon).toBeLessThanOrEqual(dx[1] + 1e-6)
      expect(lat).toBeGreaterThan(Math.min(home[0], dx[0]) - 1)
    }
    // ⚠️ THE MIDPOINT IS THE ASSERTION THAT BITES. Endpoints and those bounds are
    // survivable by a WRONG interpolation: scaling one slerp weight shifts every
    // interior point along the path, and re-normalising puts it back ON the great
    // circle — so t=0 and t=1 still land, longitude still lies between them, and a
    // real defect ships green. (Verified: poisoning the weight left this suite fully
    // passing until this check existed.) The midpoint is derived a DIFFERENT way here
    // — normalise(A + B), which is the great-circle midpoint by construction and owes
    // nothing to the slerp — so agreeing with it is evidence rather than a restatement.
    const rad = Math.PI / 180
    const vec = (p: LatLon) => {
      const f = p[0] * rad, l = p[1] * rad
      return [Math.cos(f) * Math.sin(l), Math.sin(f), Math.cos(f) * Math.cos(l)]
    }
    const A = vec(home), B = vec(dx)
    const sum = [A[0] + B[0], A[1] + B[1], A[2] + B[2]]
    const m = Math.hypot(sum[0], sum[1], sum[2])
    const midLat = Math.asin(sum[1] / m) / rad
    const midLon = Math.atan2(sum[0] / m, sum[2] / m) / rad
    expect(pts[4][0]).toBeCloseTo(midLat, 6)
    expect(pts[4][1]).toBeCloseTo(midLon, 6)

    // Antipodal-safe: a zero-length path is 9 copies of the same point.
    const same = b.globe.arcPoints(home, home, 8)
    expect(same[4][0]).toBeCloseTo(home[0], 6)
  })

  it('mirrors the shared band palette so 20m looks like 20m everywhere', () => {
    const b = boot()
    for (const band of Object.keys(BAND_COLOR)) {
      expect(b.globe.bandColor(band), band).toBe(BAND_COLOR[band])
    }
    // …including the neutral fallback for a band the palette does not name.
    expect(b.globe.bandColor('nonsense')).toBe(bandColor('nonsense'))
    // The page's copy is deliberately STRICTER than the TS helper: band labels
    // ride in on the same payload as the callsigns, so the lookup is an
    // own-property check. `BAND_COLOR['constructor'] ?? …` in bandColors.ts
    // resolves to Object; here it is the neutral fallback.
    expect(b.globe.bandColor('constructor')).toBe('#8aa0b0')
    expect(b.globe.bandColor('toString')).toBe('#8aa0b0')
  })

  it('puts the subsolar point where the sun is, and the terminator 90° from it', () => {
    const b = boot()
    // Equinox, 12:00 UTC: overhead near the equator on the Greenwich meridian.
    const eq = b.globe.subsolarPoint(Date.UTC(2026, 2, 21, 12, 0, 0))
    expect(Math.abs(eq.lat)).toBeLessThan(2)
    expect(Math.abs(eq.lon)).toBeLessThan(4)
    // Six hours later the sun has moved 90° west.
    const later = b.globe.subsolarPoint(Date.UTC(2026, 2, 21, 18, 0, 0))
    expect(later.lon).toBeCloseTo(eq.lon - 90, 3)
    // Northern solstice: overhead on the tropic of Cancer.
    const jun = b.globe.subsolarPoint(Date.UTC(2026, 5, 21, 12, 0, 0))
    expect(jun.lat).toBeGreaterThan(22)
    expect(jun.lat).toBeLessThan(24)
    // The day/night line is the great circle 90° from the sun, all the way
    // round — that is what makes it the greyline.
    const ring = b.globe.terminatorRing(Date.UTC(2026, 5, 21, 12, 0, 0), 24)
    expect(ring.length).toBe(24)
    for (const p of ring) {
      expect(angularDeg(p, [jun.lat, jun.lon])).toBeCloseTo(90, 6)
    }
  })

  it('carries its coastline inline — no request, no library, no CDN', () => {
    const b = boot()
    expect(b.globe.landRingCount()).toBe(88)
    // The page's own zero-internet guard, alongside the Rust one.
    expect(html).not.toMatch(/https?:\/\//)
    expect(html).not.toMatch(/<script[^>]+src=/i)
  })

  it('renders the newest QSO as the hero with position + operator attribution', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const hero = document.getElementById('hero')!.textContent!
    expect(hero).toContain('VE3AAA')
    expect(hero).toContain('Section 5') // section NAME resolved from meta
    expect(hero).toContain('20m FT8') // submode wins over the mode class
    expect(hero).toContain('CW tent')
    expect(hero).toContain('W9AAA')
    // The compact recent line carries the rest of the ticker.
    expect(document.getElementById('recent')!.textContent).toContain('K1ABC')
  })

  it('SFD control: the powered score block DOES show the power multiplier', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const block = document.getElementById('score-lines')!.textContent!
    expect(document.getElementById('score-total')!.textContent).toBe('162')
    expect(block).toContain(b.STRINGS.powerLine)
    expect(block).toContain('× 2')
  })

  it('WFD: raw headline + labelled projection, and NO power-multiplier text anywhere', () => {
    const b = boot()
    b.render(makeData('objectives'), makeMeta('objectives'))
    // Headline is the RAW qso points, captioned as provisional.
    expect(document.getElementById('score-total')!.textContent).toBe('6')
    expect(document.getElementById('score-caption')!.textContent).toBe(
      b.STRINGS.rawPoints,
    )
    const block = document.getElementById('score-lines')!.textContent!
    expect(block).toContain('6 × 3 → 18') // the ×(n+1) projection, labelled
    expect(block).toContain(b.STRINGS.atSubmission)
    // The no-power pin (the SFD test above is the positive control that this
    // text DOES appear when the model has a power multiplier).
    const leftPanel = document.getElementById('col-left')!.textContent!
    expect(leftPanel).not.toContain(b.STRINGS.powerLine)
    expect(leftPanel).not.toContain(b.STRINGS.poweredLine)
  })

  it('ticks claimed bonuses and leaves the rest unticked', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const claimed = [...document.querySelectorAll('#bonus-list .bonus.claimed')].map(
      (e) => e.getAttribute('data-id'),
    )
    expect(claimed.sort()).toEqual(['emergency-power', 'web-submission'])
    const unticked = document.querySelector('.bonus[data-id="safety-officer"]')!
    expect(unticked.classList.contains('claimed')).toBe(false)
    expect(unticked.querySelector('.tick')!.textContent).toBe('☐')
  })

  it('renders positions as a leaderboard with raw QSO counts and points', () => {
    const b = boot()
    b.render(makeData('powered'), makeMeta('powered'))
    const rows = [...document.querySelectorAll('#pos-list .pos-row')]
    expect(rows.length).toBe(2)
    expect(rows[0].textContent).toContain('CW tent')
    expect(rows[0].textContent).toContain('W9AAA')
    expect(rows[0].textContent).toContain('2') // raw QSOs (board shows raw)
    expect(rows[0].textContent).toContain('4') // points
  })

  it('shows the stale badge on demand and clears it on recovery', () => {
    const b = boot()
    b.renderMeta(makeMeta('powered'))
    const badge = document.getElementById('stale')!
    expect(badge.classList.contains('on')).toBe(false)
    b.setStale(true)
    expect(badge.classList.contains('on')).toBe(true)
    expect(badge.textContent).toBe(b.STRINGS.staleBadge)
    b.setStale(false)
    expect(badge.classList.contains('on')).toBe(false)
  })

  it('shows the host-only overlay for a non-host instance', () => {
    const b = boot()
    b.setInactive(true)
    const overlay = document.getElementById('inactive')!
    expect(overlay.classList.contains('on')).toBe(true)
    expect(overlay.textContent).toBe(b.STRINGS.hostOnlyMsg)
  })

  it('treats payload values as text, never markup (calls arrive over RF)', () => {
    const b = boot()
    const data = makeData('powered')
    data.ticker[0].call = '<img src=x onerror=alert(1)>'
    b.render(data, makeMeta('powered'))
    expect(document.querySelector('#hero img')).toBeNull()
    expect(document.getElementById('hero')!.textContent).toContain('<img')
  })
})
