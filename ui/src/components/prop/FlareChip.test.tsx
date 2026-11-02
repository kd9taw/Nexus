// @vitest-environment jsdom
//
// The flare chip says where every number comes from: NOAA's D-RAP grid with its valid time or "estimated
// from GOES"; NOAA's began / peak / ended with NOAA's times, or a trend word marked estimated; NOAA's own
// recovery time or NOAA's empirical fit. A simulated flare says PREVIEW and nothing of NOAA's.
//
// And the chip stack it lives in: the STACK is the positioned box, so the flare chip unfolding pushes the
// proton chip down instead of covering it (resolved from the sheet with the shared cascade resolver, on the
// elements the component actually renders — not a regex over the CSS).
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { FlareChip } from './FlareChip'
import { flareFootprint, flareNoaaOf, type FlareNoaa } from '../../flareNow'
import { flareHafAt, flareRecoveryMin } from '../../mapGeo'
import { t } from '../../i18n'
import { chainOf, parseRules, winnerAt } from '../../cssCascade'
import type { DrapGrid, FlareEvent } from '../../types'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

const NOW = Date.UTC(2026, 9, 30, 18, 0)
const NOW_S = NOW / 1000
const M5 = 5e-5

function grid(recoveryAt: number | null = null): DrapGrid {
  const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
  const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
  const haf: number[] = []
  for (const la of lats) for (const lo of lons) haf.push(flareHafAt(la, lo, NOW, M5))
  return { validAt: NOW_S - 60, recoveryAt, lats, lons, haf }
}
const event: FlareEvent = {
  at: NOW_S - 60,
  beginAt: Date.UTC(2026, 9, 30, 17, 38) / 1000,
  beginClass: 'C5.0',
  maxAt: Date.UTC(2026, 9, 30, 17, 52) / 1000,
  maxClass: 'M5.1',
  maxFlux: 5.1e-5,
  endAt: null,
  currentClass: 'M4.0',
}
const TRACE: [number, number][] = [
  [NOW_S - 3600, 2e-7],
  [NOW_S - 1800, 6e-5],
  [NOW_S - 60, 5e-5],
]
const noaa = (o: Partial<FlareNoaa> = {}): FlareNoaa => ({
  xrayAt: NOW_S - 60,
  series: TRACE,
  recent: TRACE,
  event,
  drap: grid(),
  ...o,
})

function chip(
  n: FlareNoaa | null,
  extra: { preview?: boolean; snapAt?: number | null } = {},
) {
  const foot = flareFootprint(NOW, M5, n?.drap)!
  return render(
    <div className="wx-chip-stack">
      <FlareChip
        xrayLong={M5}
        foot={foot}
        noaa={n}
        qth={{ lat: 41.9, lon: -87.6 }}
        nowMs={NOW}
        snapAt={extra.snapAt}
        preview={extra.preview}
      />
      <div className="flare-chip pca-chip" role="status">
        protons
      </div>
    </div>,
  )
}

describe('FlareChip words', () => {
  // After NOAA ends a flare the shack's 3-hour trend still says "rising" through the decay, so the
  // chip takes no word from it: "rising again" is read from the lane's own GOES trace.
  const ended: FlareEvent = { ...event, maxClass: 'M6.0', maxFlux: 6e-5, endAt: NOW_S - 300 }
  it("keeps NOAA's \"ended\" through the decay", () => {
    const decay: [number, number][] = [[NOW_S - 600, 6e-5], [NOW_S - 240, 5.5e-5], [NOW_S - 60, 5e-5]]
    const text = chip(noaa({ event: ended, recent: decay })).container.textContent ?? ''
    expect(text).toContain(t('map.flare.noaa.ended', { end: '17:55Z', begin: '17:38Z' }))
    expect(text).not.toContain(t('map.flare.est.rising'))
  })
  it('CONTROL: a trace that turns up again after the end reads Rising (estimated)', () => {
    const upAgain: [number, number][] = [[NOW_S - 600, 6e-5], [NOW_S - 240, 4e-5], [NOW_S - 60, 5e-5]]
    const text = chip(noaa({ event: ended, recent: upAgain })).container.textContent ?? ''
    expect(text).toContain(t('map.flare.est.rising'))
    expect(text).not.toContain('17:55Z')
  })

  // NOAA's event is too old to quote (over three hours ago): the direction word comes from the
  // lane's GOES trace alone (the chip no longer takes the 3-hour trend), and without a trace there
  // is none.
  const old: FlareEvent = {
    ...event,
    at: NOW_S - 60,
    beginAt: NOW_S - 4 * 3600,
    maxAt: NOW_S - 3.5 * 3600,
    maxClass: 'X2.0',
    maxFlux: 2e-4,
    endAt: NOW_S - 3 * 3600,
  }
  const stale = (recent: [number, number][]) => noaa({ event: old, recent })
  const est = (text: string) => ({
    rising: text.includes(t('map.flare.est.rising')),
    falling: text.includes(t('map.flare.est.falling')),
  })
  it('a long decay beside a stale NOAA event reads Falling, not Rising', () => {
    const decay: [number, number][] = [[NOW_S - 1500, 9e-5], [NOW_S - 900, 7e-5], [NOW_S - 300, 5.5e-5], [NOW_S - 60, 5e-5]]
    expect(est(chip(stale(decay)).container.textContent ?? '')).toEqual({ rising: false, falling: true })
  })
  it('CONTROL: a genuine rise beside a stale NOAA event reads Rising (estimated)', () => {
    const rise: [number, number][] = [[NOW_S - 1500, 2e-5], [NOW_S - 900, 2.5e-5], [NOW_S - 300, 3e-5], [NOW_S - 60, 5e-5]]
    expect(est(chip(stale(rise)).container.textContent ?? '')).toEqual({ rising: true, falling: false })
  })
  it('no trace, or an old one, gives no direction word', () => {
    const oldTrace: [number, number][] = [[NOW_S - 3000, 2e-5], [NOW_S - 2400, 5e-5], [NOW_S - 1800, 6e-5]]
    for (const recent of [[], oldTrace]) {
      expect(est(chip(stale(recent)).container.textContent ?? '')).toEqual({ rising: false, falling: false })
    }
  })

  // The lane's trace is 5-minute bucket PEAKS: its last point is the current bucket's high, up to ~4
  // minutes old. The flux the chip shows (M5 at 17:59) is the trace's newest point, so a peak at 17:55
  // that has fallen since does not read Rising beside it.
  it('reads the direction from the flux it shows, not the bucket peak before it', () => {
    const buckets: [number, number][] = [[NOW_S - 1800, 5e-6], [NOW_S - 900, 5e-6], [NOW_S - 600, 2e-5], [NOW_S - 300, 8.5e-5]]
    expect(est(chip(stale(buckets)).container.textContent ?? '')).toEqual({ rising: false, falling: true })
    // CONTROL: the bucket peak below the flux shown (still climbing) reads Rising.
    const climbing: [number, number][] = [[NOW_S - 1800, 5e-6], [NOW_S - 900, 5e-6], [NOW_S - 600, 2e-5], [NOW_S - 300, 4e-5]]
    expect(est(chip(stale(climbing)).container.textContent ?? '')).toEqual({ rising: true, falling: false })
  })

  it("names NOAA's grid and its valid time, and NOAA's event times", () => {
    const { container } = chip(noaa())
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.src.noaa', { time: '17:59Z', xtime: '17:59Z' }))
    expect(text).toContain(t('map.flare.noaa.peakedClass', { begin: '17:38Z', peak: '17:52Z', cls: 'M5.1' }))
    expect(container.querySelector('.flare-chip-src')!.getAttribute('data-source')).toBe('noaa')
  })

  it('says "estimated from GOES" when there is no current NOAA grid, and uses the trace\'s direction word', () => {
    const { container } = chip(noaa({ drap: null, event: null }))
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.src.goes', { xtime: '17:59Z' }))
    expect(text).toContain(t('map.flare.est.falling'))
    expect(text).not.toContain('NOAA D-RAP')
  })

  it('says how old the GOES sample is once it stops being now', () => {
    const { container } = chip(noaa({ drap: null, xrayAt: NOW_S - 7 * 60 }))
    expect(container.querySelector('.flare-chip-src')!.textContent).toContain('17:53Z, ')
  })

  it('unfolds to the bands, the recovery source and the sparkline', () => {
    const { container, getByRole } = chip(noaa({ drap: grid(NOW_S + 30 * 60) }))
    expect(container.querySelector('.flare-spark')).toBeNull()
    fireEvent.click(getByRole('button', { name: t('map.flare.toggle.aria') }))
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.recovery.noaa', { mins: 30 }))
    expect(container.querySelector('.flare-spark path')!.getAttribute('d')).toMatch(/^M0\.0,/)
    // NOAA's begin and peak are marked on the trace.
    expect(container.querySelectorAll('.flare-spark-mark')).toHaveLength(2)
  })

  it("falls back to NOAA's empirical fit, said so, without NOAA's recovery time", () => {
    const { container, getByRole } = chip(noaa({ drap: null }))
    fireEvent.click(getByRole('button', { name: t('map.flare.toggle.aria') }))
    expect(container.textContent).toContain(t('map.flare.recovery.fit', { mins: Math.round(flareRecoveryMin(M5)!) }))
  })

  it('a preview says PREVIEW and nothing of NOAA', () => {
    const { container } = chip(null, { preview: true })
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.phase.preview'))
    expect(text).not.toContain('NOAA')
    expect(container.querySelector('button')).toBeNull()
  })
})

describe('FlareChip: no countdown while the flare is still rising', () => {
  const open = (r: ReturnType<typeof chip>) => fireEvent.click(r.getByRole('button', { name: t('map.flare.toggle.aria') }))
  const rising: FlareEvent = { ...event, maxAt: null, maxClass: null, maxFlux: null }

  it("NOAA says it is still rising: no recovery time, NOAA's or the fit's", () => {
    const r = chip(noaa({ event: rising, drap: grid(NOW_S + 30 * 60) }))
    open(r)
    const text = r.container.textContent ?? ''
    expect(text, 'CONTROL: NOAA’s rising words are shown').toContain(t('map.flare.noaa.rising', { begin: '17:38Z' }))
    expect(text).not.toContain(t('map.flare.recovery.noaa', { mins: 30 }))
    expect(text).not.toContain(t('map.flare.recovery.fit', { mins: Math.round(flareRecoveryMin(M5)!) }))
  })

  it('POSITIVE CONTROL: once it has peaked the countdown is there', () => {
    const r = chip(noaa({ drap: grid(NOW_S + 30 * 60) }))
    open(r)
    expect(r.container.textContent).toContain(t('map.flare.recovery.noaa', { mins: 30 }))
  })
})

describe("FlareChip: the class's GOES time on the snapshot path", () => {
  const src = (r: ReturnType<typeof chip>) => r.container.querySelector('.flare-chip-src')!.textContent
  it("with no fast lane, quotes the snapshot's sample time", () => {
    expect(src(chip(null, { snapAt: NOW_S - 120 }))).toBe(t('map.flare.src.goes', { xtime: '17:58Z' }))
  })
  it("the fast lane's own time wins when it is there", () => {
    expect(src(chip(noaa({ drap: null }), { snapAt: NOW_S - 600 }))).toBe(t('map.flare.src.goes', { xtime: '17:59Z' }))
  })
  it("the dev override's simulated flux has no sample time", () => {
    localStorage.setItem('nexus.dev.xray', String(M5))
    expect(src(chip(null, { snapAt: NOW_S - 120 }))).toBe(t('map.flare.src.goesOnly'))
  })
})

describe('the chip stack in the cascade', () => {
  const RAW = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8')
  const RULES = parseRules(RAW.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')))

  it('positions the stack, not each chip, so an unfolded chip pushes the next one down', () => {
    const { container } = chip(noaa())
    const stack = container.querySelector('.wx-chip-stack')!
    const flare = container.querySelector('.flare-chip:not(.pca-chip)')!
    const pca = container.querySelector('.pca-chip')!
    expect(winnerAt(RULES, 'dark', chainOf(stack), 'position')?.value).toBe('absolute')
    for (const el of [flare, pca]) {
      expect(winnerAt(RULES, 'dark', chainOf(el), 'position')?.value ?? 'static').toBe('static')
      expect(winnerAt(RULES, 'dark', chainOf(el), 'top')).toBeNull()
    }
  })

  it('draws the proton chip with its own violet edge, not the flare orange', () => {
    const { container } = chip(noaa())
    const pca = container.querySelector('.pca-chip')!
    const win = winnerAt(RULES, 'dark', chainOf(pca), 'border-left', 'border-left-color')
    expect(win?.value).toContain('#a855f7')
  })
})

// The lane's `series` is 5-minute bucket PEAKS (for the sparkline), and peaks hide the troughs: when a
// decay turns up again, the dip's bucket keeps its higher sample on the rising side, so a word read
// from the peaks says "Falling" for minutes while the flux printed beside it climbs from the dip. The
// direction word reads the lane's raw one-minute samples (`recent`). These tests build the lane as the
// shack does (propagation `xray_series` / `xray_recent`, from the same GOES file) and read the word
// off the rendered chip, with the clock slid through every minute of a bucket.
describe('FlareChip: the direction word reads the raw GOES minutes, not the bucket peaks', () => {
  type Pt = [number, number]
  const BASE = Date.UTC(2026, 9, 31, 18, 0) / 1000 // a bucket boundary
  /** The shack's `xray_series`: each 5-minute bucket's peak, with its own time. */
  const bucket = (s: Pt[]): Pt[] => {
    const out: Pt[] = []
    for (const p of s) {
      const l = out[out.length - 1]
      if (l && Math.floor(l[0] / 300) === Math.floor(p[0] / 300)) {
        if (p[1] > l[1]) out[out.length - 1] = p
      } else out.push(p)
    }
    return out
  }
  /** The lane at `nowS` from GOES's minute samples: the newest one, the bucket peaks and the raw
   *  minutes of the 70 minutes before it, as `get_xray_now` sends them. */
  const laneAt = (samples: Pt[], nowS: number, event: FlareEvent | null) => {
    const seen = samples.filter(([t]) => t <= nowS)
    const [asOf, flux] = seen[seen.length - 1]
    const recent = seen.filter(([t]) => t >= asOf - 70 * 60)
    return { flux, noaa: flareNoaaOf({ asOf, series: bucket(seen), recent, event, drap: null }, false)! }
  }
  /** The chip's direction word for that lane, read off the rendered chip. */
  const wordAt = (samples: Pt[], nowS: number, event: FlareEvent | null = null) => {
    const { flux, noaa } = laneAt(samples, nowS, event)
    const foot = flareFootprint(nowS * 1000, flux, null)
    expect(foot, 'the chip shows only at M1+').not.toBeNull()
    const { container, unmount } = render(
      <FlareChip xrayLong={flux} foot={foot!} noaa={noaa} qth={null} nowMs={nowS * 1000} />,
    )
    const text = container.textContent ?? ''
    unmount()
    const rising = text.includes(t('map.flare.est.rising'))
    const falling = text.includes(t('map.flare.est.falling'))
    expect(rising && falling).toBe(false)
    return rising ? 'rising' : falling ? 'falling' : null
  }
  /** NOAA's list still on a flare that began 22 minutes ago and was last updated 12 minutes ago:
   *  too old to quote, so the chip falls back to the trace. */
  const staleList = (nowS: number): FlareEvent => ({
    at: nowS - 12 * 60,
    beginAt: nowS - 22 * 60,
    beginClass: 'C5.0',
    maxAt: null,
    maxClass: null,
    maxFlux: null,
    endAt: null,
    currentClass: 'M2.0',
  })

  it('a decay that turns up again reads Rising once the flux is past the margin above the dip, never Falling', () => {
    const misses: string[] = []
    for (const pct of [2, 3, 5, 8, 12]) {
      // The dip at every minute of a bucket.
      for (let off = 0; off < 5; off++) {
        const dip = BASE - 20 * 60 + off * 60
        const f = (t: number) => {
          const dm = (t - dip) / 60
          return dm <= 0 ? 2e-5 * Math.pow(1.03, -dm) : 2e-5 * Math.pow(1 + pct / 100, dm)
        }
        const samples: Pt[] = []
        for (let s = dip - 60 * 60; s <= dip + 18 * 60; s += 60) samples.push([s, f(s)])
        for (let k = 1; k <= 18; k++) {
          const nowS = dip + k * 60
          const up = f(nowS) / f(dip)
          const w = wordAt(samples, nowS)
          // Past the margin above the dip it is Rising (the rule's own threshold, from the first
          // minute it is crossed); at or below it, never Rising, and never Falling once it climbs.
          if (up > 1.1 && w !== 'rising') misses.push(`${pct}%/min dip@${off} +${k}m up ${up.toFixed(3)}: ${w}`)
          if (up <= 1.1 && w === 'rising') misses.push(`${pct}%/min dip@${off} +${k}m Rising within the margin`)
        }
      }
    }
    expect(misses).toEqual([])
  })

  it('CONTROL: a plain decay reads Falling at every minute', () => {
    const samples: Pt[] = []
    for (let s = BASE - 80 * 60; s <= BASE; s += 60) samples.push([s, 3e-4 * Math.pow(0.97, (s - BASE + 80 * 60) / 60)])
    for (let k = 0; k < 10; k++) expect(wordAt(samples, BASE - k * 60)).toBe('falling')
  })

  // Every shape, the clock slid one minute at a time through two buckets, with no event and with a
  // stale list. The word must not contradict the flux printed: Rising is wrong when the flux shown fell
  // from the minute before and is more than the margin below the highest of the four minutes before
  // it; Falling is wrong when it rose from the minute before and is more than the margin above their
  // lowest. A minute below M1 has no chip.
  const lin = (knots: Array<[number, number]>) => (m: number) => {
    for (let k = 0; k < knots.length - 1; k++) {
      const [m0, f0] = knots[k]
      const [m1, f1] = knots[k + 1]
      if (m <= m0 && m >= m1) return f0 + ((m0 - m) / (m0 - m1)) * (f1 - f0)
    }
    return knots[knots.length - 1][1]
  }
  const shapes: Record<string, (minsBefore: number) => number> = {
    impulsive_peak_m5: (m) => (m > 10 ? 5e-6 : m >= 5 ? 5e-6 + ((10 - m) / 5) * 4.5e-5 : 5e-5 * Math.exp(-(5 - m) / 8)),
    impulsive_peak_m8: (m) => (m > 13 ? 5e-6 : m >= 8 ? 5e-6 + ((13 - m) / 5) * 4.5e-5 : 5e-5 * Math.exp(-(8 - m) / 8)),
    impulsive_peak_m3: (m) => (m > 8 ? 5e-6 : m >= 3 ? 5e-6 + ((8 - m) / 5) * 4.5e-5 : 5e-5 * Math.exp(-(3 - m) / 8)),
    double_peak: lin([[40, 5e-6], [30, 5e-6], [24, 5e-5], [16, 2e-5], [9, 4e-5], [0, 2.6e-5]]),
    triple_peak: lin([[40, 5e-6], [28, 5e-5], [23, 1.5e-5], [18, 4e-5], [13, 2e-5], [9, 3.5e-5], [0, 2.2e-5]]),
    climb: lin([[40, 5e-6], [12, 5e-6], [0, 6e-5]]),
    gradual_rise: (m) => 1e-5 * Math.exp((40 - m) / 30),
    rise_noisy_2pct: (m) => 1e-5 * Math.exp((40 - m) / 30) * (m % 2 ? 1.02 : 0.98),
    slow_decay: (m) => 4e-5 * Math.exp(-(40 - m) / 40),
    decay_noisy_2pct: (m) => 4e-5 * Math.exp(-(40 - m) / 40) * (m % 2 ? 1.02 : 0.98),
    flat: () => 1e-5,
    rerise_in_decay: lin([[40, 6e-5], [10, 2e-5], [0, 3.5e-5]]),
    decay_spike_15pct: (m) => 4e-5 * Math.exp(-(40 - m) / 40) * (m === 6 ? 1.15 : 1),
    decay_dropout_15pct: (m) => 4e-5 * Math.exp(-(40 - m) / 40) * (m === 6 ? 0.85 : 1),
  }
  for (const [name, f] of Object.entries(shapes)) {
    it(`sliding clock, ${name}: an honest word or none at every minute`, () => {
      const samples: Pt[] = []
      for (let m = 40; m >= 0; m--) samples.push([BASE - m * 60, f(m)])
      const bad: string[] = []
      for (let k = 0; k < 10; k++) {
        const nowS = BASE - k * 60
        const seen = samples.filter(([t]) => t <= nowS)
        const [at, shown] = seen[seen.length - 1]
        if (shown < 1e-5) continue
        const prev = seen.filter(([t]) => t < at && t >= at - 240).map(([, x]) => x)
        const step = shown - seen[seen.length - 2][1]
        for (const ev of [null, staleList(nowS)]) {
          const w = wordAt(samples, nowS, ev)
          if (w === 'rising' && step < 0 && shown * 1.1 < Math.max(...prev)) bad.push(`-${k}m ${ev ? 'stale' : 'none'}: Rising`)
          if (w === 'falling' && step > 0 && shown > Math.min(...prev) * 1.1) bad.push(`-${k}m ${ev ? 'stale' : 'none'}: Falling`)
        }
      }
      expect(bad).toEqual([])
    })
  }

  it('the rerise shape reads Rising from the first minute the flux is past the margin above its dip', () => {
    // 10 minutes ago the decay bottomed out at 2e-5; it has climbed linearly to 3.5e-5 since.
    const f = shapes.rerise_in_decay
    const samples: Pt[] = []
    for (let m = 40; m >= 0; m--) samples.push([BASE - m * 60, f(m)])
    for (let k = 0; k < 10; k++) {
      const up = f(k) / 2e-5
      if (up > 1.1) expect(wordAt(samples, BASE - k * 60), `-${k}m`).toBe('rising')
    }
  })

  it('a station that sends no raw minutes gives no direction word, never the bucket peaks\' word', () => {
    const decay: [number, number][] = [[NOW_S - 1500, 9e-5], [NOW_S - 900, 7e-5], [NOW_S - 300, 5.5e-5]]
    const old = flareNoaaOf({ asOf: NOW_S - 60, series: decay, event: null, drap: null }, false)!
    const text = chip(old).container.textContent ?? ''
    expect(text).not.toContain(t('map.flare.est.falling'))
    expect(text).not.toContain(t('map.flare.est.rising'))
  })
})

describe('FlareChip: where on the sun (NOAA’s active regions)', () => {
  const DAY = Date.UTC(2026, 9, 29) / 1000
  const regions = {
    held: true,
    why: 'held',
    observedDay: DAY,
    until: DAY + 3 * 86_400,
    regions: [
      {
        region: 4564,
        latDeg: 5,
        lonDeg: 68,
        location: 'N05E68',
        magClass: 'A',
        spotClass: 'Axx',
        area: 10,
        numberSpots: 1,
        mFlareProbability: 1,
        xFlareProbability: 1,
      },
    ],
  }
  const foot = flareFootprint(NOW, M5, null)!

  it('the unfolded chip carries the sun disc with the held regions', () => {
    const r = render(<FlareChip xrayLong={M5} foot={foot} noaa={noaa()} qth={null} nowMs={NOW} regions={regions} />)
    expect(r.container.querySelector('.sun-regions'), 'CONTROL: folded, no disc').toBeNull()
    fireEvent.click(r.container.querySelector('.flare-chip-head')!)
    expect(r.container.querySelector('[data-region="4564"]')).not.toBeNull()
  })

  it('a stale list puts no region on the disc, and says so', () => {
    const stale = { ...regions, held: false, why: 'stale', until: null, regions: [] }
    const r = render(<FlareChip xrayLong={M5} foot={foot} noaa={noaa()} qth={null} nowMs={NOW} regions={stale} />)
    fireEvent.click(r.container.querySelector('.flare-chip-head')!)
    expect(r.container.querySelector('[data-region]')).toBeNull()
    expect(r.container.querySelector('.sun-regions-none')!.getAttribute('data-why')).toBe('stale')
  })
})
