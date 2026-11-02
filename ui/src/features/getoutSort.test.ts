// @vitest-environment jsdom
//
// The Getting Out list's sort (features/getoutSort): the four orders, their tie-breaks, and the choice
// each box keeps across a restart — validated on load, so a value this build does not know reads as
// Distance.
import { describe, it, expect, beforeEach } from 'vitest'
import { GETOUT_SORTS, loadGetoutSort, saveGetoutSort, sortGetout } from './getoutSort'
import type { HeardMe } from '../types'

const KEY = 'nexus.connect.getout.sort'

const r = (call: string, km: number, snr: number | null, ageSecs: number): HeardMe => ({
  call,
  grid: null,
  band: '20m',
  snr,
  bearingDeg: 0,
  km,
  octant: 'N',
  ageSecs,
})

const A = r('K1AA', 900, -12, 300)
const B = r('DL2BB', 7000, -20, 60)
const C = r('JA3CC', 10000, -5, 600)
const D = r('VK4DD', 15000, null, 30)
const calls = (rows: HeardMe[]) => rows.map((x) => x.call)

beforeEach(() => localStorage.clear())

describe('the four orders', () => {
  it('offers Distance, Call, Newest and SNR, Distance first', () => {
    expect(GETOUT_SORTS).toEqual(['distance', 'call', 'newest', 'snr'])
  })

  it('Distance: most distant first, as the station sends them', () => {
    expect(calls(sortGetout([A, B, C, D], 'distance'))).toEqual(['VK4DD', 'JA3CC', 'DL2BB', 'K1AA'])
  })

  it('Call: A to Z', () => {
    expect(calls(sortGetout([A, B, C, D], 'call'))).toEqual(['DL2BB', 'JA3CC', 'K1AA', 'VK4DD'])
  })

  it('Newest: the most recent report first', () => {
    expect(calls(sortGetout([A, B, C, D], 'newest'))).toEqual(['VK4DD', 'DL2BB', 'K1AA', 'JA3CC'])
  })

  it('SNR: the strongest first, a report without an SNR last', () => {
    expect(calls(sortGetout([D, A, B, C], 'snr'))).toEqual(['JA3CC', 'K1AA', 'DL2BB', 'VK4DD'])
  })

  it('never reorders the list it is given', () => {
    const rows = [A, B, C, D]
    sortGetout(rows, 'call')
    expect(calls(rows)).toEqual(['K1AA', 'DL2BB', 'JA3CC', 'VK4DD'])
  })
})

describe('ties keep a stable order', () => {
  it('Distance: an equal distance goes by call', () => {
    const rows = [r('W9ZZ', 5000, -10, 60), r('G4AA', 5000, -10, 60), r('F5MM', 8000, -10, 60)]
    expect(calls(sortGetout(rows, 'distance'))).toEqual(['F5MM', 'G4AA', 'W9ZZ'])
  })

  it('Newest: an equal age goes by distance, then call', () => {
    const rows = [r('W9ZZ', 5000, -10, 60), r('G4AA', 5000, -10, 60), r('F5MM', 8000, -10, 60), r('EA1A', 100, 0, 10)]
    expect(calls(sortGetout(rows, 'newest'))).toEqual(['EA1A', 'F5MM', 'G4AA', 'W9ZZ'])
  })

  it('SNR: an equal SNR goes by distance, then call — reports without one too', () => {
    const rows = [r('W9ZZ', 5000, -10, 60), r('G4AA', 5000, -10, 60), r('F5MM', 8000, -10, 60), r('ON4X', 100, null, 5), r('OH2Y', 900, null, 5)]
    expect(calls(sortGetout(rows, 'snr'))).toEqual(['F5MM', 'G4AA', 'W9ZZ', 'OH2Y', 'ON4X'])
  })

  it('Call: the same call twice goes by distance', () => {
    const rows = [r('K1AA', 100, -10, 60), r('K1AA', 9000, -10, 60)]
    expect(sortGetout(rows, 'call').map((x) => x.km)).toEqual([9000, 100])
  })
})

describe('the choice each box keeps', () => {
  it('is Distance until chosen', () => {
    expect(loadGetoutSort('left1')).toBe('distance')
  })

  it('a restart brings back what each box chose, apart from the others', () => {
    saveGetoutSort('left1', 'snr')
    saveGetoutSort('box2', 'call')
    expect(loadGetoutSort('left1')).toBe('snr')
    expect(loadGetoutSort('box2')).toBe('call')
    expect(loadGetoutSort('rail1')).toBe('distance')
  })

  it('a value this build does not know reads as Distance', () => {
    localStorage.setItem(KEY, JSON.stringify({ left1: 'loudest', box2: 7, rail1: 'newest' }))
    expect(loadGetoutSort('left1')).toBe('distance')
    expect(loadGetoutSort('box2')).toBe('distance')
    expect(loadGetoutSort('rail1')).toBe('newest')
  })

  it('a record that does not parse, or is not a record, reads as Distance and is written over cleanly', () => {
    localStorage.setItem(KEY, '{not json')
    expect(loadGetoutSort('left1')).toBe('distance')
    saveGetoutSort('left1', 'newest')
    expect(loadGetoutSort('left1')).toBe('newest')
    localStorage.setItem(KEY, JSON.stringify(['snr']))
    expect(loadGetoutSort('0')).toBe('distance')
  })

  it('a name inherited from the object prototype is not a choice', () => {
    expect(loadGetoutSort('toString')).toBe('distance')
    expect(loadGetoutSort('__proto__')).toBe('distance')
  })
})
