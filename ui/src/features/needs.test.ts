import { describe, expect, it } from 'vitest'
import { modeClassOf, tagsForSurface, visibleNeeds, workTarget } from './needs'
import type { BandChannel, NeedAlert, NeedTag } from '../types'

function alert(call: string, mode: string, band = '20m', freqMhz: number | null = null): NeedAlert {
  return {
    call,
    entity: 'Test',
    band,
    zone: 14,
    tags: ['NewEntity'],
    priority: 100,
    headline: 'New one',
    mode,
    freqMhz,
  }
}

const BAND_PLAN: BandChannel[] = [
  { band: '20m', group: 'HF', dialMhz: 14.074, mode: 'USB', label: '20m', note: '' },
  { band: '40m', group: 'HF', dialMhz: 7.074, mode: 'USB', label: '40m', note: '' },
]

describe('visibleNeeds', () => {
  const all = [alert('A', 'Digital'), alert('B', 'CW'), alert('C', 'Phone')]

  it('digital op (modes off) sees only digital needs — board unchanged', () => {
    const v = visibleNeeds(all, { cw: false, phone: false })
    expect(v.map((a) => a.call)).toEqual(['A'])
  })

  it('CW enabled surfaces CW needs; Phone still hidden', () => {
    const v = visibleNeeds(all, { cw: true, phone: false })
    expect(v.map((a) => a.call)).toEqual(['A', 'B'])
  })

  it('both modes on shows everything', () => {
    expect(visibleNeeds(all, { cw: true, phone: true }).length).toBe(3)
  })

  it('an unknown mode class defaults to visible (fail-open, never hide a need)', () => {
    const v = visibleNeeds([alert('Z', 'SSTV')], { cw: false, phone: false })
    expect(v.length).toBe(1)
  })
})

describe('workTarget', () => {
  it('CW need with an exact spot freq → QSY there, cw view', () => {
    const t = workTarget(alert('3Y0J', 'CW', '20m', 14.025), BAND_PLAN)
    expect(t).toEqual({ call: '3Y0J', view: 'cw', freqMhz: 14.025, band: '20m' })
  })

  it('Phone need opens the phone cockpit at the spot frequency', () => {
    const t = workTarget(alert('EA5DX', 'Phone', '20m', 14.25), BAND_PLAN)
    expect(t).toEqual({ call: 'EA5DX', view: 'phone', freqMhz: 14.25, band: '20m' })
  })

  it('CW need with no exact freq → the band CW activity freq, NOT the FT8 dial', () => {
    // Regression: a freq-less CW/Phone need used to fall back to the tier dial (14.074 = FT8),
    // which sent CW/phone click-to-work to an FT8 frequency. Now it lands in the CW window.
    const t = workTarget(alert('JA1XYZ', 'CW', '20m', null), BAND_PLAN)
    expect(t?.freqMhz).toBe(14.03)
    const p = workTarget(alert('EA5', 'Phone', '20m', null), BAND_PLAN)
    expect(p?.freqMhz).toBe(14.25)
  })

  it('a Digital need QSYs to the exact spot freq and opens the digital cockpit (N1MM-style)', () => {
    const t = workTarget(alert('A', 'Digital', '20m', 14.074), BAND_PLAN)
    expect(t).toEqual({ call: 'A', view: 'operate', freqMhz: 14.074, band: '20m' })
  })

  it('a Digital need with no spot freq falls back to the band default channel', () => {
    const t = workTarget(alert('A', 'Digital', '40m', null), BAND_PLAN)
    expect(t).toEqual({ call: 'A', view: 'operate', freqMhz: 7.074, band: '40m' })
  })

  it('no frequency resolvable (unknown band, no spot freq) → null', () => {
    expect(workTarget(alert('A', 'CW', '60m', null), BAND_PLAN)).toBeNull()
  })

  it('an RTTY need routes to the rtty cockpit at the exact spot freq', () => {
    const t = workTarget(alert('DL1RT', 'RTTY', '20m', 14.085), BAND_PLAN)
    expect(t).toEqual({ call: 'DL1RT', view: 'rtty', freqMhz: 14.085, band: '20m' })
  })
})

describe('modeClassOf (map-spot → cockpit routing)', () => {
  it('CW routes to the CW cockpit', () => {
    expect(modeClassOf('CW')).toBe('CW')
    expect(modeClassOf('cw')).toBe('CW')
  })
  it('voice modes AND the "Phone" class label route to Phone', () => {
    // Both ADIF tokens and our own class label — a need alert's mode is the LABEL "Phone",
    // which previously fell through to Digital and routed a phone need to the wrong cockpit.
    for (const m of ['SSB', 'USB', 'LSB', 'FM', 'AM', 'ssb', 'Phone', 'PHONE']) {
      expect(modeClassOf(m)).toBe('Phone')
    }
  })
  it('digital + unknown + missing route to Digital (fail-safe)', () => {
    for (const m of ['FT8', 'FT4', 'RTTY', 'PSK31', 'JS8', 'weird', '', null, undefined]) {
      expect(modeClassOf(m)).toBe('Digital')
    }
  })
})

// ---------------------------------------------------------------------------
// Field report 2026-07-29: the Needed system claimed a "new mode on 30m" for
// Asiatic Russia on an operator who has six 30m FT8 contacts with that entity.
// The backend need was real (they have never worked it on CW) but the roster
// chips are keyed by CALL alone, so a CW alert painted an unqualified MODE chip
// onto their 30m FT8 surface. tagsForSurface is the gate those maps were missing.
// ---------------------------------------------------------------------------
describe('tagsForSurface (the false "new mode" gate)', () => {
  const ar = (tags: NeedTag[], mode: string, band = '30m'): NeedAlert => ({
    call: 'RF9C',
    entity: 'Asiatic Russia',
    band,
    zone: 17,
    tags,
    priority: 30,
    headline: 'New mode — CW Asiatic Russia',
    mode,
    freqMhz: null,
  })

  it('drops a CW new-mode need from a 30m FT8 surface (the operator report)', () => {
    expect(tagsForSurface(ar(['NewMode'], 'CW'), '30m', 'FT8')).toEqual([])
  })

  it('keeps a digital new-mode need on a digital surface', () => {
    expect(tagsForSurface(ar(['NewMode'], 'FT8'), '30m', 'FT8')).toEqual(['NewMode'])
  })

  it('matches the backend submode vocabulary against a class label', () => {
    // The backend sends mode: 'FT8'/'FT4'/'RTTY' verbatim for digital rows, but the
    // decode feed describes itself as the class 'Digital'. Raw === never matched.
    for (const m of ['FT8', 'FT4', 'RTTY', 'Digital']) {
      expect(tagsForSurface(ar(['NewMode'], m), '30m', 'Digital')).toEqual(['NewMode'])
    }
  })

  it('folds band-label case (a real log carries both 30M and 30m)', () => {
    expect(tagsForSurface(ar(['NewBand'], 'FT8', '30m'), '30M', 'FT8')).toEqual(['NewBand'])
    expect(tagsForSurface(ar(['NewBand'], 'FT8', ' 30M '), '30m', 'FT8')).toEqual(['NewBand'])
  })

  it('keeps an all-time-new entity on any band and any mode', () => {
    expect(tagsForSurface(ar(['NewEntity'], 'CW', '20m'), '30m', 'FT8')).toEqual(['NewEntity'])
  })

  it('drops a cross-band new-band claim', () => {
    expect(tagsForSurface(ar(['NewBand'], 'FT8', '20m'), '30m', 'FT8')).toEqual([])
  })

  it('keeps a same-class new-mode need scored on another band (the predicate is entity-wide)', () => {
    // worked_mode is keyed (entity, mode-class) with NO band, so a digital mode need
    // is closable on any band — including this one.
    expect(tagsForSurface(ar(['NewMode'], 'FT8', '20m'), '30m', 'FT8')).toEqual(['NewMode'])
  })

  it('drops a cross-band confirmation (confirmed_band IS per band)', () => {
    expect(tagsForSurface(ar(['Confirm'], 'FT8', '20m'), '30m', 'FT8')).toEqual([])
  })

  it('keeps program flags regardless of band or mode', () => {
    expect(tagsForSurface(ar(['Pota', 'Sota', 'Dxped', 'Wanted'], 'CW', '20m'), '30m', 'FT8')).toEqual(
      ['Pota', 'Sota', 'Dxped', 'Wanted'],
    )
  })
})
