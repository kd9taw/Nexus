// @vitest-environment jsdom
//
// THE RTTY CHANNEL LIST FOLLOWS THE KEYING BACKEND. Each row comes at the dial the station's
// backend tunes for it (`Engine::rtty_channel_plan`): on AFSK a mark tone above the plan's FSK
// dial. The cockpit stays mounted across navigation and fetched the list once, so a backend
// switched in Settings left the other backend's dials in it: picked on AFSK from a list fetched on
// FSK, the 20 m mark went out at 14.0809, inside FT4. The list is fetched again when the backend
// changes.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  rttyAutoArm: vi.fn(),
  getRttyScope: vi.fn(async () => ({ live: true, seq: 1, peak: 0.4, xy: [], offsetHz: null })),
  getLicensedBandPlan: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyAutoArm = api.rttyAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const ARMED = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.085125, band: '20m', catOk: true, sideband: 'LSB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

const settle = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

afterEach(cleanup)

describe('the RTTY channel list', () => {
  it('is fetched again when the keying backend changes', async () => {
    let backend = 'afsk'
    // The backend in force at each fetch of the list.
    const fetchedOn: string[] = []
    getRttyState.mockImplementation(async () => ({ ...ARMED, backend }))
    rttyAutoArm.mockImplementation(async () => ({ ...ARMED, backend }))
    getLicensedBandPlan.mockImplementation(async () => {
      fetchedOn.push(backend)
      return []
    })
    render(<RttyCockpit snap={snap} onSetTxEnabled={() => {}} />)
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
    await settle(50)
    expect(fetchedOn, 'premise: the list was fetched on AFSK').toContain('afsk')
    backend = 'fsk' // switched in Settings while the cockpit stays mounted
    await settle(700) // the next state poll (500 ms) reads it
    expect(fetchedOn[fetchedOn.length - 1], `the backend at each fetch: ${fetchedOn.join(', ')}`).toBe('fsk')
  })
})
