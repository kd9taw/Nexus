import { describe, it, expect } from 'vitest'
import { gridCountsToPoints, qsoGridCounts, qsoGridPoints } from './qsoPoints'
import { gridToLatLon } from '../grid'
import type { LoggedQso } from '../types'

function q(p: Partial<LoggedQso>): LoggedQso {
  return {
    call: 'W1AW',
    grid: 'FN31',
    band: '20m',
    freqMhz: 14.074,
    mode: 'FT8',
    rstSent: '-10',
    rstRcvd: '-10',
    whenUnix: 1_700_000_000,
    confirmed: false,
    awardConfirmed: false,
    ...p,
  }
}

describe('qsoGridPoints', () => {
  it('dedupes to one point per 4-char square, carrying the QSO count', () => {
    const pts = qsoGridPoints([q({ grid: 'FN31pr' }), q({ grid: 'FN31aa' }), q({ grid: 'FN31' })], 'all')
    expect(pts).toHaveLength(1)
    expect(pts[0].n).toBe(3)
  })

  it("colours a square by its MOST-RECENT QSO's band", () => {
    const pts = qsoGridPoints(
      [
        q({ grid: 'EM64', band: '40m', whenUnix: 100 }),
        q({ grid: 'EM64', band: '2m', whenUnix: 200 }), // newer → wins the colour
      ],
      'all',
    )
    expect(pts).toHaveLength(1)
    expect(pts[0].band).toBe('2m')
  })

  it('band filter shows only that band (VUCC: grids are per-band)', () => {
    const log = [q({ grid: 'FN31', band: '20m' }), q({ grid: 'EM64', band: '2m' })]
    expect(qsoGridPoints(log, 'all')).toHaveLength(2)
    const two = qsoGridPoints(log, '2m')
    expect(two).toHaveLength(1)
    expect(two[0].band).toBe('2m')
  })

  it('skips grid-less or too-short grids (no fabricated points)', () => {
    expect(qsoGridPoints([q({ grid: null }), q({ grid: '' }), q({ grid: 'FN' })], 'all')).toEqual([])
  })

  it('maps a square to a plausible lat/lng', () => {
    const [p] = qsoGridPoints([q({ grid: 'FN31' })], 'all')
    expect(p.lat).toBeGreaterThan(40)
    expect(p.lat).toBeLessThan(42)
    expect(p.lng).toBeGreaterThan(-74)
    expect(p.lng).toBeLessThan(-72)
  })
})

// The reduction was split in two (SPEC-2 v3 C17b): the counted squares are what a `LogSource`
// answers, the placing stays in the UI. ORACLE is the one-piece function as it stood
// (qsoPoints.ts at d5be14ea), pasted unchanged; the composition must equal it.
describe('qsoGridPoints split into counting and placing', () => {
  function ORACLE(qsos: LoggedQso[], band: string) {
    const acc = new Map<string, { n: number; band: string; when: number }>()
    for (const q of qsos) {
      if (band !== 'all' && q.band !== band) continue
      const gr = (q.grid ?? '').trim().toUpperCase()
      if (gr.length < 4) continue
      const key = gr.slice(0, 4)
      const cur = acc.get(key)
      if (cur) {
        cur.n += 1
        if (q.whenUnix >= cur.when) {
          cur.when = q.whenUnix
          cur.band = q.band
        }
      } else {
        acc.set(key, { n: 1, band: q.band, when: q.whenUnix })
      }
    }
    const pts: { lat: number; lng: number; n: number; band: string }[] = []
    acc.forEach((v, gr) => {
      const ll = gridToLatLon(gr)
      if (ll) pts.push({ lat: ll.lat, lng: ll.lon, n: v.n, band: v.band })
    })
    return pts
  }

  it('equals the one-piece reduction on mixed input, every band filter', () => {
    const log = [
      q({ grid: 'FN31pr', band: '20m', whenUnix: 5 }),
      q({ grid: ' fn31 ', band: '40m', whenUnix: 5 }), // equal time: the later row wins the colour
      q({ grid: 'EM64', band: '2m', whenUnix: 9 }),
      q({ grid: 'ZZ99', band: '2m', whenUnix: 1 }), // a square the parser cannot place
      q({ grid: 'FN', band: '20m' }),
      q({ grid: null, band: '20m' }),
    ]
    for (const band of ['all', '20m', '40m', '2m', '6m']) {
      expect(qsoGridPoints(log, band), band).toEqual(ORACLE(log, band))
      expect(gridCountsToPoints(qsoGridCounts(log, band)), band).toEqual(ORACLE(log, band))
    }
    expect(qsoGridCounts(log, 'all').map((c) => c.grid)).toEqual(['FN31', 'EM64', 'ZZ99'])
  })
})
