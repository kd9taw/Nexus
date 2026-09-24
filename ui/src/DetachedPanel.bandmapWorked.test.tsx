// @vitest-environment jsdom
//
// THE BAND-MAP POP-OUT STRIKES THROUGH EXACTLY THE CALLS IN THE LOG (SPEC-2 v3 C17b, V2).
//
// The pop-out used to hold the whole log to answer one question per spot: "is this call in my
// log?". It now asks `LogSource` which of the calls ON THE MAP are worked. The rule must not move:
// a spot is worked when its call, upper-cased, equals some logged call upper-cased — UNTRIMMED, the
// same rule the band map has always used (v2 §4 `worked_calls`). This pins that rule on the real
// pop-out against the rule computed straight from the log, and that a contact logged while the map
// is up strikes its spot through on the next tick.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import type { AppSnapshot, LoggedQso, SpotRow } from './types'

const station = vi.hoisted(() => ({
  log: [] as unknown[],
  revision: 1,
  snap: null as unknown,
  listeners: new Set<(s: unknown) => void>(),
  logReads: 0,
}))

vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  out.subscribeSnapshot = vi.fn((cb: (s: unknown) => void) => {
    station.listeners.add(cb)
    cb(station.snap)
    return () => station.listeners.delete(cb)
  })
  out.getBandPlan = vi.fn().mockResolvedValue([])
  out.getNeedAlerts = vi.fn().mockResolvedValue([])
  out.getSettings = vi.fn().mockResolvedValue({})
  out.getPropagation = vi.fn().mockResolvedValue(null)
  out.getAllSpots = vi.fn(() => Promise.resolve(SPOTS))
  out.getLogDelta = vi.fn(async (since: number, have: number) => {
    station.logReads++
    return since === 0
      ? { revision: station.revision, full: true, rows: station.log.slice() }
      : { revision: station.revision, full: false, rows: station.log.slice(have) }
  })
  return out
})

const snapAt = (logTick: number) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    logTick,
    stations: [],
    conversations: [],
    activePeer: null,
    recentDecodes: [],
    hunt: null,
    link: { tier: 'ft8' },
    radio: { band: '20m', dialMhz: 14.03, txAllowed: true, phoneSegLo: null, phoneSegHi: null, sideband: 'USB', catOk: true, txBusyReason: null, transmitting: false },
  }) as unknown as AppSnapshot

const spot = (call: string, freqMhz: number): SpotRow =>
  ({ call, entity: '', zone: 0, band: '20m', freqMhz, mode: 'CW', spotter: 'K3LR', corroborators: [], ageSecs: 30, comment: '' }) as unknown as SpotRow

/** On the map: a logged call, one logged in another case, one logged WITH a trailing space (so the
 *  spot without it is NOT worked), the spot that carries the space, one never worked, and one the
 *  test logs while the map is up. */
const SPOTS = [
  spot('W1AW', 14.01),
  spot('w1abc', 14.02),
  spot('DL1ABC', 14.03),
  spot('DL1ABC ', 14.04),
  spot('K2XYZ', 14.05),
  spot('N0NEW', 14.06),
]
const logged = (call: string) => ({ call, band: '20m', freqMhz: 14.02, mode: 'CW', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false }) as unknown as LoggedQso

/** The rule, straight from the log: what the pop-out must strike through. */
const expectedWorked = (log: LoggedQso[]) => {
  const inLog = new Set(log.map((q) => q.call.toUpperCase()))
  return SPOTS.filter((s) => inLog.has(s.call.toUpperCase())).map((s) => s.call).sort()
}
const struckThrough = (root: HTMLElement) =>
  [...root.querySelectorAll('.bandmap-spot.worked .bandmap-call')].map((el) => el.textContent ?? '').sort()
const shown = (root: HTMLElement) => root.querySelectorAll('.bandmap-spot').length

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  station.log = [logged('W1AW'), logged('W1ABC'), logged('DL1ABC '), logged('K9OLD')]
  station.revision = 1
  station.snap = snapAt(1)
  station.logReads = 0
})
afterEach(cleanup)

describe('the band-map pop-out’s worked calls', () => {
  it('strikes through exactly the spots the log holds (upper-cased, untrimmed), with one log read', async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await waitFor(() => expect(shown(container)).toBe(SPOTS.length))
    await waitFor(() => expect(struckThrough(container)).toEqual(expectedWorked(station.log as LoggedQso[])))
    // The fixture must reach the rule's corners, or the equality above proves little.
    expect(struckThrough(container)).toEqual(['DL1ABC ', 'W1AW', 'w1abc'])
    expect(station.logReads).toBe(1)
  })

  it('a contact logged while the map is up is struck through on the next tick', async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await waitFor(() => expect(struckThrough(container)).toEqual(['DL1ABC ', 'W1AW', 'w1abc']))
    station.log = [...station.log, logged('N0NEW')]
    station.revision = 2
    station.snap = snapAt(2)
    act(() => {
      for (const l of station.listeners) l(station.snap)
    })
    await waitFor(() => expect(struckThrough(container)).toEqual(['DL1ABC ', 'N0NEW', 'W1AW', 'w1abc']))
  })
})
