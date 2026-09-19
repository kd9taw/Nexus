// @vitest-environment jsdom
//
// #335, a poller's half: the CW cockpit reads its decode every 200 ms, and `cw_decode` takes the
// engine mutex — the one held across blocking CAT I/O. A bare interval stacked a read per tick
// behind a stall (fifteen in three seconds), each holding a backend worker; the read is
// single-flight now (singleFlight.ts). This pins it through the real cockpit, so the wiring —
// not just the helper — is what goes red if a poller is put back on a bare interval.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'

const decode = vi.hoisted(() => ({
  pending: [] as ((v: unknown) => void)[],
  answer: {
    text: 'CQ DE K1ABC', wpm: 22, sent: [] as string[], keyerError: null, candidates: [], state: 'listening',
    headline: '', prompt: '', recommended: null, workedCall: null, rst: null, name: null,
  },
}))
const cwDecode = vi.hoisted(() => vi.fn(() => new Promise((resolve) => decode.pending.push(resolve))))

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (see CwCockpit.agc.test.tsx for why); the decode
  // read is the one under test, and it answers only when told.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    previewCw: vi.fn(async (t: string) => t),
    cwDecode,
  }
})
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
    txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null,
    nb: true, nr: true, notch: null, filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
  },
} as unknown as AppSnapshot

beforeEach(() => {
  vi.useFakeTimers()
  decode.pending = []
  cwDecode.mockClear()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the CW cockpit reads its decode one call at a time (#335)', () => {
  it('asks once in 3 s while a read is stalled, and resumes when it answers', async () => {
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await act(() => vi.advanceTimersByTimeAsync(3000))
    expect(cwDecode, 'a read stacked on every 200 ms tick behind the stall').toHaveBeenCalledTimes(1)

    await act(async () => {
      decode.pending[0](decode.answer)
      await vi.advanceTimersByTimeAsync(0)
    })
    await act(() => vi.advanceTimersByTimeAsync(200))
    expect(cwDecode).toHaveBeenCalledTimes(2)
  })
})
