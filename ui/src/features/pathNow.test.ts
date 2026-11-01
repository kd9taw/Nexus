import { describe, expect, it } from 'vitest'
import type { PathHourOverlay, SpaceNow } from '../types'
import { flareFootprint, flareUnderSun } from '../flareNow'
import {
  bandNowOf,
  clockZ,
  nowClauses,
  nowDetail,
  nowHit,
  nowHitKind,
  overlaySegments,
  spaceNowLines,
} from './pathNow'

/** 2026-05-15 12:00 UTC. */
const NOW = 1_778_846_400

function overlay(over: Partial<PathHourOverlay> = {}): PathHourOverlay {
  return {
    unix: NOW,
    kp: 3,
    kpSource: 'measured',
    capLatDeg: 62.4,
    source: {},
    bands: [],
    ...over,
  }
}

const NOAA_FLARE = overlay({
  source: {
    xrayAt: NOW - 60,
    xrayLong: 1e-4,
    haf: 'noaa',
    drapValidAt: NOW - 45,
    recovery: { at: NOW + 42 * 60, source: 'noaa' },
  },
  bands: [
    { band: '40m', hops: 2, hopsModelled: true, flareDb: 79.9, segments: [{ from: 0.4, to: 1, kind: 'flareOut' }] },
    { band: '20m', hops: 2, hopsModelled: true, flareDb: 8.6, segments: [{ from: 0.2, to: 1, kind: 'flareWeak' }] },
    { band: '10m', hops: 2, hopsModelled: false, flareDb: 0.4 },
  ],
})

const GOES_FLARE = overlay({
  source: { xrayAt: NOW - 120, xrayLong: 1e-4, haf: 'goes', recovery: { at: NOW + 58 * 60, source: 'ruleOfThumb' } },
  bands: [{ band: '20m', hops: 3, hopsModelled: true, flareDb: 12.3 }],
})

const S2 = overlay({
  source: { protonsAt: NOW - 300, j10: 150, sScale: 2 },
  bands: [{ band: '20m', hops: 3, hopsModelled: true, pcaDb: 6.2, segments: [{ from: 0.3, to: 0.6, kind: 'pca' }] }],
})

describe('pathNow — the flare and proton clauses on a path', () => {
  it('says the estimate, its NOAA source and time, and NOAA’s own recovery time', () => {
    const c = nowClauses(NOAA_FLARE, '20m', NOW)
    expect(c).toHaveLength(1)
    expect(c[0]).toContain('~9 dB of flare absorption on 20m')
    expect(c[0]).toContain("estimated from NOAA's D-RAP grid 11:59Z")
    expect(c[0]).toContain("12:42Z (NOAA's own estimate)")
  })

  it('names where each band’s figure came from when the split took the GOES estimate', () => {
    const at = (flareFrom: 'noaa' | 'goes' | 'both') =>
      nowClauses({ ...NOAA_FLARE, source: { ...NOAA_FLARE.source, xrayAt: NOW - 180, flareCapGeomag: 50 }, bands: [{ band: '20m', hops: 2, hopsModelled: true, flareDb: 8.6, flareFrom }] }, '20m', NOW).join(' ')
    // Wholly from our GOES estimate: GOES with its time, never "NOAA's D-RAP grid".
    expect(at('goes')).toContain('(estimated from GOES 11:57Z)')
    expect(at('goes')).not.toContain('D-RAP')
    // Partly: both, each with its own time.
    expect(at('both')).toContain("(estimated from NOAA's D-RAP grid 11:59Z and GOES 11:57Z)")
    // Where the grid stood throughout, NOAA's grid.
    expect(at('noaa')).toContain("(estimated from NOAA's D-RAP grid 11:59Z)")
    expect(at('noaa')).not.toContain('and GOES')
  })

  it('never names NOAA for the GOES estimate, and names the rule of thumb as such', () => {
    const c = nowClauses(GOES_FLARE, '20m', NOW).join(' ')
    expect(c).toContain('~12 dB')
    expect(c).toContain('estimated from GOES 11:58Z')
    expect(c).not.toContain('D-RAP')
    expect(c).toContain("NOAA's rule of thumb from the X-ray level")
    expect(c).not.toContain("NOAA's own estimate")
  })

  it('caps a band the flare takes out instead of quoting a meaningless figure', () => {
    const c = nowClauses(NOAA_FLARE, '40m', NOW).join(' ')
    expect(c).toContain('more than 30 dB of flare absorption on 40m, out')
    expect(c).not.toContain('80')
  })

  it('says nothing below 1 dB, for a band it has no figure for, or without an overlay', () => {
    expect(nowClauses(NOAA_FLARE, '10m', NOW)).toEqual([])
    expect(nowClauses(NOAA_FLARE, '6m', NOW)).toEqual([])
    expect(nowClauses(NOAA_FLARE, null, NOW)).toEqual([])
    expect(nowClauses(undefined, '20m', NOW)).toEqual([])
  })

  it('words a proton event with its S level and the GOES protons’ time', () => {
    const c = nowClauses(S2, '20m', NOW).join(' ')
    expect(c).toContain('~6 dB of polar-cap absorption on 20m')
    expect(c).toContain('S2 proton event')
    expect(c).toContain('GOES protons 11:55Z')
  })

  it('a proton figure needs NOAA’s S level and the sample’s time to speak', () => {
    const noScale = overlay({ source: { protonsAt: NOW - 300 }, bands: S2.bands })
    expect(nowClauses(noScale, '20m', NOW)).toEqual([])
    const noTime = overlay({ source: { sScale: 2 }, bands: S2.bands })
    expect(nowClauses(noTime, '20m', NOW)).toEqual([])
    expect(nowClauses(S2, '20m', NOW)).toHaveLength(1)
  })

  it('the strip’s hatch is the proton event’s when only the proton figure speaks', () => {
    const both = overlay({
      source: { ...S2.source, xrayAt: NOW - 60, xrayLong: 1.2e-5, haf: 'goes' },
      bands: [{ band: '20m', hops: 3, hopsModelled: true, flareDb: 0.4, pcaDb: 6.2 }],
    })
    expect(nowHitKind(both, '20m')).toBe('pca')
    expect(nowHitKind(NOAA_FLARE, '20m')).toBe('flare')
    expect(nowHitKind(NOAA_FLARE, '10m')).toBeNull()
  })

  it('the detail rows say how many crossings and whether the hops are the model’s', () => {
    const d = nowDetail(NOAA_FLARE, '20m').join('\n')
    expect(d).toContain('D-region crossings: 4')
    expect(d).toContain("hops: 2, the model's mode this hour")
    // No proton event: nothing about the grid's polar part.
    expect(d).not.toContain('polar-cap absorption')
    // The split took GOES on this route. With a PCA figure the proton part is counted once …
    const splitSrc = { ...NOAA_FLARE.source, flareCapGeomag: 50 }
    const counted = nowDetail({ ...NOAA_FLARE, source: splitSrc, bands: [{ band: '20m', hops: 2, hopsModelled: true, flareDb: 8.6, pcaDb: 12 }] }, '20m').join('\n')
    expect(counted).toContain('Poleward of 50° geomagnetic the flare figure takes no more than the GOES X-ray estimate')
    expect(counted).toContain('counted once, as polar-cap absorption')
    // … and without one (no Kp) it is said to be counted nowhere, never "once".
    const uncounted = nowDetail({ ...NOAA_FLARE, source: splitSrc }, '20m').join('\n')
    expect(uncounted).toContain('Poleward of 50° geomagnetic')
    expect(uncounted).toContain('is not counted here')
    // The split runs only during a held proton event, so the only reason left is the missing Kp.
    expect(uncounted).toContain('no Kp reading')
    expect(uncounted).not.toContain('S1+')
    expect(uncounted).not.toContain('counted once')
    const a = nowDetail(NOAA_FLARE, '10m').join('\n')
    expect(a).toContain('assumed')
    expect(nowDetail(undefined, '20m')).toEqual([])
  })

  it('picks the band’s overlay and its ribbon stretches; nothing for another band', () => {
    expect(bandNowOf(NOAA_FLARE, '20m')?.flareDb).toBe(8.6)
    expect(bandNowOf(NOAA_FLARE, '6m')).toBeNull()
    expect(overlaySegments(NOAA_FLARE, '40m').map((s) => s.kind)).toEqual(['flareOut'])
    expect(overlaySegments(S2, '20m').map((s) => s.kind)).toEqual(['pca'])
    expect(overlaySegments(NOAA_FLARE, null)).toEqual([])
  })

  it('the strip’s now cell is marked only at 1 dB or more', () => {
    expect(nowHit(NOAA_FLARE, '20m', NOW)).toContain('~9 dB of flare absorption')
    expect(nowHit(NOAA_FLARE, '10m', NOW)).toBeNull()
    expect(nowHit(S2, '20m', NOW)).toContain('polar-cap')
    expect(nowHit(undefined, '20m', NOW)).toBeNull()
  })

  it('times are UTC clock times, dated when not today', () => {
    expect(clockZ(NOW + 3 * 60, NOW)).toBe('12:03Z')
    expect(clockZ(NOW + 86_400, NOW)).toMatch(/16.*12:00Z|12:00Z/)
    expect(clockZ(NOW + 86_400, NOW)).not.toBe('12:00Z')
  })
})

describe('spaceNowLines — the outlook’s and the strip’s "now" line', () => {
  it('names the bands out under the sun and until when, with the source and its time', () => {
    const s: SpaceNow = {
      flare: {
        r: 3,
        xrayLong: 1e-4,
        xrayAt: NOW - 180,
        haf: 'noaa',
        drapValidAt: NOW - 45,
        hafSubMhz: 30,
        out: '60m',
        weak: '10m',
        recovery: { at: NOW + 80 * 60, source: 'noaa' },
      },
    }
    const [line] = spaceNowLines(s, NOW)
    expect(line).toContain('Flare X1.0 (R3): 60m and below out on the dayside')
    expect(line).toContain("until ~13:20Z (NOAA's own estimate)")
    // NOAA's HAF with its valid time, and the GOES sample behind the class with its own.
    expect(line).toContain('NOAA D-RAP 11:59Z · GOES 11:57Z')
  })

  it('a GOES estimate is labelled as one, and a weakened-only flare says weakened', () => {
    const s: SpaceNow = {
      flare: {
        r: 1,
        xrayLong: 1.2e-5,
        xrayAt: NOW - 60,
        haf: 'goes',
        hafSubMhz: 15.8,
        weak: '20m',
        recovery: { at: NOW + 27 * 60, source: 'ruleOfThumb' },
      },
    }
    const [line] = spaceNowLines(s, NOW)
    expect(line).toContain('20m and below weakened on the dayside')
    expect(line).toContain('estimated from GOES 11:59Z')
    expect(line).not.toContain('D-RAP')
    expect(line).toContain("rule of thumb")
  })

  it('carries a proton event and a NOAA watch, ahead or in force, and nothing when quiet', () => {
    const lines = spaceNowLines(
      { protons: { s: 2, j10: 150, at: NOW - 300 }, watch: { g: 1, from: NOW + 12 * 3600, to: NOW + 36 * 3600, issued: NOW - 3600 } },
      NOW,
    )
    expect(lines[0]).toContain('S2 proton event')
    expect(lines[0]).toContain('GOES 11:55Z')
    // NOAA's issue time travels with the watch.
    expect(lines[1]).toContain('G1 watch (NOAA, issued 11:00Z) from')
    const inForce = spaceNowLines({ watch: { g: 2, from: NOW - 3600, to: NOW + 12 * 3600, issued: NOW - 86_400 } }, NOW)
    // Issued the day before: dated.
    expect(inForce[0]).toMatch(/^G2 watch \(NOAA, issued \S+ \S+ 12:00Z\) in force until /)
    expect(spaceNowLines(undefined, NOW)).toEqual([])
    expect(spaceNowLines({}, NOW)).toEqual([])
  })

  it('agrees with the map chip: X1 from GOES takes out 60m and weakens to 12m (the shack’s fixture)', () => {
    // The Rust test `the_now_line_names_the_bands_out_under_the_sun_and_until_when` pins the same
    // two bands for the same flux, so the line and the chip's "Under the sun" cannot drift apart.
    const foot = flareFootprint(NOW * 1000, 1e-4, null)
    expect(foot && flareUnderSun(foot)).toEqual({ out: '60m', weak: '12m' })
  })
})
