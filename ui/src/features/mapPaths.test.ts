// The TX/RX path-line selection rules. Pure — no canvas, no jsdom: what is under test is
// WHICH relationships earn a line and how many, which is the whole legibility argument.
// (The geometry itself is `mapGeo.greatCircle()` + `geoPath`, already shipping the
// selected-path line in all three projections.)
import { describe, it, expect } from 'vitest'
import type { MapSpot, Station } from '../types'
import { MAX_PATHS, TX_MAX_AGE_SECS, txPaths, rxPaths } from './mapPaths'

const spot = (call: string, ageSecs: number, heardMe = true, grid = { lat: 48, lon: 11 }): MapSpot => ({
  call,
  lat: grid.lat,
  lon: grid.lon,
  band: '20m',
  heardMe,
  ageSecs,
  approx: false,
})

const station = (
  call: string,
  lastHeardSlot: number,
  presence: Station['presence'] = 'active',
  grid: string | null = 'JN58',
): Station => ({
  call,
  grid,
  snr: -12,
  lastHeardSlot,
  heardCount: 1,
  presence,
  worked: false,
})

describe('txPaths — stations that reported hearing ME', () => {
  it('takes only heard-me spots, freshest first', () => {
    const out = txPaths([spot('DL1AAA', 300), spot('W1BBB', 60), spot('F5CCC', 120, false)])
    expect(out.map((p) => p.call)).toEqual(['W1BBB', 'DL1AAA'])
    expect(out.every((p) => p.dir === 'tx')).toBe(true)
  })

  it('drops a path older than the recency gate rather than fading it forever', () => {
    const out = txPaths([spot('OLD', TX_MAX_AGE_SECS + 1), spot('NEW', TX_MAX_AGE_SECS - 1)])
    expect(out.map((p) => p.call)).toEqual(['NEW'])
  })

  it('fades with age: under 10 min full, beyond it dimmer', () => {
    const out = txPaths([spot('FRESH', 60), spot('AGING', 20 * 60)])
    expect(out[0].fade).toBe(1)
    expect(out[1].fade).toBeLessThan(1)
  })

  it('caps the fan so a pileup cannot web the map', () => {
    const many = Array.from({ length: MAX_PATHS + 25 }, (_, i) => spot(`W${i}AA`, i))
    expect(txPaths(many)).toHaveLength(MAX_PATHS)
  })
})

describe('rxPaths — stations I decoded', () => {
  it('takes gridded roster stations, freshest first', () => {
    const out = rxPaths([station('DL1AAA', 10), station('W1BBB', 40), station('NOGRID', 99, 'active', null)])
    expect(out.map((p) => p.call)).toEqual(['W1BBB', 'DL1AAA'])
    expect(out.every((p) => p.dir === 'rx')).toBe(true)
  })

  it('drops a stale station outright and dims an idle one', () => {
    const out = rxPaths([station('GONE', 5, 'stale'), station('IDLE', 6, 'idle'), station('LIVE', 7)])
    expect(out.map((p) => p.call)).toEqual(['LIVE', 'IDLE'])
    expect(out[0].fade).toBe(1)
    expect(out[1].fade).toBeLessThan(1)
  })

  it('caps the fan like the TX side', () => {
    const many = Array.from({ length: MAX_PATHS + 25 }, (_, i) => station(`W${i}AA`, i))
    expect(rxPaths(many)).toHaveLength(MAX_PATHS)
  })

  it('yields a two-way call to its TX line — one stroke, not two over the same arc', () => {
    const tx = txPaths([spot('BOTH', 60)])
    const out = rxPaths([station('both', 10), station('W1ONLY', 11)], tx)
    expect(out.map((p) => p.call)).toEqual(['W1ONLY'])
  })
})
