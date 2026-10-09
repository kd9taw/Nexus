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
import type { LogQuestion } from './features/logAnswers'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from './features/notAnswered'

const station = vi.hoisted(() => ({
  log: [] as unknown[],
  revision: 1,
  snap: null as unknown,
  listeners: new Set<(s: unknown) => void>(),
  logReads: 0,
  /** Spots the feed adds to SPOTS. */
  extraSpots: [] as unknown[],
  /** Questions that ask about a call here are held until `answerHeld`, or refused with `failing`. */
  holding: new Set<string>(),
  failing: null as string | null,
  held: [] as (() => void)[],
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
  out.getAllSpots = vi.fn(() => Promise.resolve([...SPOTS, ...station.extraSpots]))
  // The engine answers each question the pop-out asks, over its log.
  const { answerAs } = await import('./features/logAnswers.testkit')
  out.askLog = vi.fn(async (q: LogQuestion) => {
    station.logReads++
    if (q.kind === 'workedCalls' && q.calls.some((c) => station.holding.has(c))) {
      if (station.failing !== null) throw station.failing
      return new Promise((resolve) => station.held.push(() => resolve(answerAs(q, station.log as LoggedQso[], station.revision))))
    }
    return answerAs(q, station.log as LoggedQso[], station.revision)
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
  station.extraSpots = []
  station.holding = new Set()
  station.failing = null
  station.held = []
})
afterEach(cleanup)

describe('the band-map pop-out’s worked calls', () => {
  it('strikes through exactly the spots the log holds (upper-cased, untrimmed), asking only about the map’s calls', async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await waitFor(() => expect(shown(container)).toBe(SPOTS.length))
    await waitFor(() => expect(struckThrough(container)).toEqual(expectedWorked(station.log as LoggedQso[])))
    // The fixture must reach the rule's corners, or the equality above proves little.
    expect(struckThrough(container)).toEqual(['DL1ABC ', 'W1AW', 'w1abc'])
    // What it asked the engine: which of the calls on the map are worked — nothing else, and never
    // the log itself.
    const { askLog } = await import('./api')
    const asked = vi.mocked(askLog).mock.calls.map(([q]) => q)
    expect(station.logReads).toBe(asked.length)
    expect(asked.map((q) => q.kind).filter((k) => k !== 'workedCalls')).toEqual([])
    const onMap = new Set(SPOTS.map((s) => s.call.toUpperCase()))
    expect(asked.flatMap((q) => (q.kind === 'workedCalls' ? q.calls : [])).filter((c) => !onMap.has(c.toUpperCase()))).toEqual([])
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

// A NEW CALL ON THE MAP COSTS IT NO STRIKE-THROUGH. The question is keyed on the calls on the map,
// so a spot poll that brings a new call asks a new one. Until its answer landed, every worked spot
// lost its strike-through: the new question had no answer, and the map drew none worked. Now the
// calls the last answer covered keep it, and only a call no answer has covered yet shows "—".
describe('a new call on the map', () => {
  const K1NEW = spot('K1NEW', 14.07)
  const WORKED = ['DL1ABC ', 'W1AW', 'w1abc']
  /** The spots marked "—" (not answered yet), by call, and their tooltips. */
  const unanswered = (root: HTMLElement) =>
    [...root.querySelectorAll('.bandmap-spot')].flatMap((b) => {
      const mark = [...b.querySelectorAll('.bandmap-call')].find((c) => c.textContent === '—')
      return mark ? [[b.querySelector('.bandmap-call')!.textContent, mark.getAttribute('title')]] : []
    })
  const flush = () => act(() => vi.advanceTimersByTimeAsync(0))
  /** The feed's next poll, 15 s on. */
  const poll = () => act(() => vi.advanceTimersByTimeAsync(15_000))
  const answerHeld = () =>
    act(async () => {
      for (const answer of station.held.splice(0)) answer()
      await vi.advanceTimersByTimeAsync(0)
    })
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('keeps every strike-through while the new question is on its way, and marks only the new call', async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await flush()
    expect([struckThrough(container), unanswered(container)], 'premise').toEqual([WORKED, []])
    station.holding.add('K1NEW')
    station.extraSpots = [K1NEW]
    await poll()
    expect(shown(container), 'premise: the new spot is on the map').toBe(SPOTS.length + 1)
    expect(struckThrough(container), 'the answer it had').toEqual(WORKED)
    expect(unanswered(container)).toEqual([['K1NEW', 'Reading the logbook…']])
    await answerHeld()
    expect([struckThrough(container), unanswered(container)], 'answered: K1NEW is not in the log').toEqual([WORKED, []])
  })

  it('a question that fails leaves the new call "—", saying why, and the rest as they were', async () => {
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await flush()
    station.holding.add('K1NEW')
    station.failing = 'database disk image is malformed'
    station.extraSpots = [K1NEW]
    await poll()
    expect(struckThrough(container)).toEqual(WORKED)
    expect(unanswered(container)).toEqual([['K1NEW', 'Couldn’t read the logbook: database disk image is malformed.']])
  })

  it('a question refused while a change is being saved keeps "Reading the logbook…" on the new call, after every ask', async () => {
    const { askLog } = await import('./api')
    const { container } = render(<DetachedPanel panel="bandmapCw" />)
    await flush()
    vi.mocked(askLog).mockClear()
    station.holding.add('K1NEW')
    station.failing = `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
    station.extraSpots = [K1NEW]
    await poll()
    for (let i = 0; i < ASK_AGAIN_TIMES; i++) await act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
    const refused = vi.mocked(askLog).mock.calls.filter(([q]) => q.kind === 'workedCalls' && q.calls.includes('K1NEW'))
    expect(refused.length, 'premise: every ask refused').toBe(1 + ASK_AGAIN_TIMES)
    expect(struckThrough(container)).toEqual(WORKED)
    expect(unanswered(container)).toEqual([['K1NEW', 'Reading the logbook…']])
  })
})
