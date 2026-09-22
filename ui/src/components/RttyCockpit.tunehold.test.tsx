// @vitest-environment jsdom
//
// #230, the path the label missed — A TUNE CARRIER HOLDS THE PICTURE TOO.
//
// The shipped half of #230 labels the frozen waterfall while we key, because on a surface that
// paints no dark band a held picture is indistinguishable from a dead one (W9GTY's report, and
// `Waterfall.tx.test.tsx` pins the component). The label is driven by each cockpit's OWN keyed
// state, and RTTY/PSK/JS8/SSTV each computed that from their message-sending flags alone.
//
// The backend hold does not: `service.rs` sets it from
// `tx_until_ms.is_some() || tuning_keyed || manual_ptt_applied`, so pressing Tune freezes the
// waterfall exactly as an over does — with no label, which is the reported symptom, on the
// surface the report is about. Tune reached the RTTY and PSK cockpits on 2026-08-20, after the
// #230 label was written.
//
// `snap.radio.tuning` is the same fact the backend gates on ("whether a tune carrier is
// currently keyed", types.ts), so the cockpit and the hold cannot disagree.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, cleanup } from '@testing-library/react'
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
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  rttyNet: vi.fn(),
  haltTx: vi.fn(),
  getSpectrumRow: vi.fn(),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyAutoArm = api.rttyAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const getSpectrumRow = api.getSpectrumRow as ReturnType<typeof vi.fn>

const ARMED: RttyState = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  markHz: 2125,
  spaceHz: 2295,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
}

const snapWith = (over: Record<string, unknown>) =>
  ({
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.08,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: true,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  getRttyState.mockReset().mockResolvedValue(ARMED)
  rttyAutoArm.mockReset().mockImplementation(() => api.getRttyState())
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  getSpectrumRow.mockReset().mockResolvedValue({ row: [], loHz: 200, hiHz: 4000 })
})
afterEach(cleanup)

const heldLabel = () => screen.queryByText(/transmitting/i)

describe('RTTY: the held waterfall says so during a tune too (#230)', () => {
  it('a keyed tune carrier gets the held-display label', async () => {
    render(<RttyCockpit snap={snapWith({ tuning: true })} />)
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
    expect(heldLabel(), 'the tune carrier freezes the waterfall with nothing saying so').not.toBeNull()
  })

  it('control: idle, not tuning, not sending — no label', async () => {
    render(<RttyCockpit snap={snapWith({})} />)
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
    expect(heldLabel(), 'a live waterfall must not claim to be held').toBeNull()
  })

  it('control: an ordinary over still gets it (the path that already worked)', async () => {
    getRttyState.mockResolvedValue({ ...ARMED, sending: true })
    render(<RttyCockpit snap={snapWith({})} />)
    await waitFor(() => expect(heldLabel()).not.toBeNull())
  })
})
