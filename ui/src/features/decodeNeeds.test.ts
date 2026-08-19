import { describe, it, expect } from 'vitest'
import { resolveDecodeNeeds, isAwardNeed } from './decodeNeeds'
import type { DecodeRow, NeedAlert, NeedTag } from '../types'

function decode(over: Partial<DecodeRow> = {}): DecodeRow {
  return {
    from: 'DL1ABC',
    snr: -10,
    dtSec: 0.1,
    freqHz: 1200,
    message: 'CQ DL1ABC JO31',
    isCq: true,
    directedToMe: false,
    worked: false,
    tier: 'FT8',
    rv: 0,
    ...over,
  }
}

function alert(tags: NeedTag[], band = '20m', over: Partial<NeedAlert> = {}): NeedAlert {
  return {
    call: 'DL1ABC',
    entity: 'Germany',
    band,
    zone: 14,
    tags,
    priority: 50,
    headline: '',
    mode: 'Digital',
    freqMhz: null,
    ...over,
  }
}

describe('resolveDecodeNeeds', () => {
  it('uses the decode-native newDxcc flag with no alerts', () => {
    const r = resolveDecodeNeeds(decode({ newDxcc: true }), '20m', [])
    expect(r.cats).toEqual(['entity'])
    expect(r.rowNeed).toBe('need-entity')
  })

  it('never throws on empty alerts and returns the native grid need', () => {
    const r = resolveDecodeNeeds(decode({ newGrid: true }), '20m', [])
    expect(r.cats).toEqual(['grid'])
    expect(r.rowNeed).toBe('need-grid')
  })

  it('honours a band-matched NewBand but ignores another band', () => {
    const same = resolveDecodeNeeds(decode(), '20m', [alert(['NewBand'], '20m')])
    expect(same.cats).toContain('band')
    const other = resolveDecodeNeeds(decode(), '20m', [alert(['NewBand'], '40m')])
    expect(other.cats).not.toContain('band')
  })

  it('treats NewEntity / Dxped as band-agnostic, but NewZone as per-band', () => {
    // entity is all-time (band-agnostic) and dxped is a station property, so a 40m alert
    // still tags them on a 20m decode — but zones are judged PER BAND (5BWAZ), so a 40m
    // NewZone must NOT paint a zone pill on the 20m row (the cross-band false-pill fix).
    const r = resolveDecodeNeeds(decode(), '20m', [alert(['NewEntity', 'NewZone', 'Dxped'], '40m')])
    expect(r.cats).toEqual(expect.arrayContaining(['entity', 'dxped']))
    expect(r.cats).not.toContain('zone')
  })

  it('orders cats by precedence and picks the top award for the row colour', () => {
    const r = resolveDecodeNeeds(decode(), '20m', [
      alert(['NewBand', 'NewEntity', 'NewZone', 'Confirm'], '20m'),
    ])
    expect(r.cats).toEqual(['entity', 'zone', 'band', 'confirm'])
    expect(r.rowNeed).toBe('need-entity')
  })

  it('renders dxped/pota/sota as icons but never as the row colour', () => {
    const r = resolveDecodeNeeds(decode({ from: 'VP6X' }), '20m', [alert(['Dxped', 'Pota'], '20m')])
    expect(r.cats).toEqual(expect.arrayContaining(['dxped', 'pota']))
    expect(r.rowNeed).toBeNull()
  })

  it('confirm-only colours need-confirm, which is NOT an award need (ranks below CQ)', () => {
    const r = resolveDecodeNeeds(decode(), '20m', [alert(['Confirm'], '20m')])
    expect(r.cats).toEqual(['confirm'])
    expect(r.rowNeed).toBe('need-confirm')
    expect(isAwardNeed(r.rowNeed)).toBe(false)
    expect(isAwardNeed('need-entity')).toBe(true)
    expect(isAwardNeed(null)).toBe(false)
  })

  it('excludes mode-specific needs from a different mode (a CW need on the FT8 feed)', () => {
    const cw = alert(['NewMode', 'Confirm'], '20m', { mode: 'CW' })
    const r = resolveDecodeNeeds(decode({ worked: true }), '20m', [cw], 'Digital')
    expect(r.cats).not.toContain('mode')
    expect(r.cats).not.toContain('confirm')
    // No false award nudge — the row stays worked/B4-dimmable, not painted need-mode.
    expect(r.rowNeed).toBeNull()
  })

  it('keeps NewBand regardless of mode (a band-slot closes in any mode)', () => {
    const cwBand = alert(['NewBand'], '20m', { mode: 'CW' })
    const r = resolveDecodeNeeds(decode(), '20m', [cwBand], 'Digital')
    expect(r.cats).toContain('band')
  })

  it('returns all applicable cats (the component caps the displayed icon count)', () => {
    const r = resolveDecodeNeeds(decode({ newDxcc: true, newGrid: true }), '20m', [
      alert(['NewBand', 'NewMode', 'Confirm'], '20m'),
    ])
    expect(r.cats.length).toBeGreaterThanOrEqual(4)
    // Precedence preserved.
    expect(r.cats[0]).toBe('entity')
  })
})

// The mirror of the false-positive: the backend sends the SPECIFIC submode as the
// display label ('FT8'/'FT4'/'RTTY') while the decode feed describes itself by class
// ('Digital'). The old raw `a.mode === feedMode` was never true for those rows, so every
// genuine NewMode/Confirm pill was silently dropped on the digital feed.
describe('mode-class + band normalization on the decode feed', () => {
  for (const m of ['FT8', 'FT4', 'RTTY', 'Digital']) {
    it(`keeps a mode-gated need whose label is '${m}' on the Digital feed`, () => {
      const r = resolveDecodeNeeds(decode(), '20m', [alert(['NewMode'], '20m', { mode: m })])
      expect(r.cats).toContain('mode')
    })
  }

  it('still withholds a CW mode need from the digital feed', () => {
    const r = resolveDecodeNeeds(decode(), '20m', [alert(['NewMode'], '20m', { mode: 'CW' })])
    expect(r.cats).not.toContain('mode')
  })

  it('folds band-label case — a real log carries both 20m and 20M', () => {
    const r = resolveDecodeNeeds(decode(), '20M', [alert(['NewBand'], '20m')])
    expect(r.cats).toContain('band')
  })
})

// The operator's report is about the DECODE FEED as much as the roster, and the feed's
// grid/entity icons come from the decode's OWN engine flags — not only from the alerts.
// Gating the alerts alone leaves the icon exactly where he saw it.
describe('the alert band scopes gate the decode-native flags too', () => {
  const DEFAULTS = { dxcc: 'all', grid: 'vhf', rareGrid: 'vhf' }

  it('no scopes → unchanged (the flag alone still tags)', () => {
    expect(resolveDecodeNeeds(decode({ newGrid: true }), '20m', []).cats).toEqual(['grid'])
  })

  it('HF + grid scope VHF+ → no GRID icon', () => {
    const r = resolveDecodeNeeds(decode({ newGrid: true }), '20m', [], 'Digital', DEFAULTS)
    expect(r.cats).toEqual([])
    expect(r.rowNeed).toBeNull()
  })

  it('POSITIVE CONTROL — 6 m keeps the GRID icon', () => {
    const r = resolveDecodeNeeds(decode({ newGrid: true }), '6m', [], 'Digital', DEFAULTS)
    expect(r.cats).toEqual(['grid'])
  })

  it('a rare grid follows the RARE scope', () => {
    const gem = decode({ newGrid: true, gridRarity: 'ultraRare' })
    expect(resolveDecodeNeeds(gem, '20m', [], 'Digital', DEFAULTS).cats).toEqual([])
    expect(
      resolveDecodeNeeds(gem, '20m', [], 'Digital', { ...DEFAULTS, rareGrid: 'all' }).cats,
    ).toEqual(['grid'])
  })

  it('the DXCC scope gates the native ATNO flag (set it to vhf — it defaults to all)', () => {
    const atno = decode({ newDxcc: true })
    expect(resolveDecodeNeeds(atno, '20m', [], 'Digital', DEFAULTS).cats).toEqual(['entity'])
    const hfOff = { ...DEFAULTS, dxcc: 'vhf' }
    expect(resolveDecodeNeeds(atno, '20m', [], 'Digital', hfOff).cats).toEqual([])
    expect(resolveDecodeNeeds(atno, '6m', [], 'Digital', hfOff).cats).toEqual(['entity'])
  })

  it('a new BAND keeps its icon on HF — only the scoped kinds are withheld', () => {
    const r = resolveDecodeNeeds(
      decode({ newGrid: true }),
      '20m',
      [alert(['NewBand'], '20m')],
      'Digital',
      DEFAULTS,
    )
    expect(r.cats).toEqual(['band'])
  })

  it('an unresolvable band stays permissive', () => {
    expect(resolveDecodeNeeds(decode({ newGrid: true }), '', [], 'Digital', DEFAULTS).cats).toEqual(
      ['grid'],
    )
  })
})
