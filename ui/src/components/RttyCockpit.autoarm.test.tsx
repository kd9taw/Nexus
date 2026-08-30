// @vitest-environment jsdom
//
// RTTY was the ONLY decode mode with no auto-arm. PSK arms on entering its view
// (`PskCockpit.tsx`, driven by `psk_rx_auto_arm`, default true), APRS and SSTV likewise; RTTY's
// decoder could be started in exactly one place — the Arm RX button inside the `stream` pane,
// which is also the one pane an operator can hide. That is very likely what the "RTTY is not
// decoding" reports were.
//
// ⚠️ WHAT THIS FILE DOES AND DOES NOT PROVE. It exercises the UI half ONLY: that entering the
// view asks the engine to auto-arm, once per entry, and that the answer is rendered. The
// BACKEND half is wave 2 — `Engine::rtty_auto_arm` and the `rtty_auto_arm` Tauri command do not
// exist yet — so the real round trip is DEAD until they land, and a green run here is not
// evidence that RTTY auto-arms on a real radio. Nothing below mocks its way around that; the
// api verb is mocked exactly as every other verb in this cockpit's tests is.
//
// The POLICY is deliberately not tested here because it is deliberately not here: only
// upgrading from off, the session decline memory and the `rttyRxAutoArm` opt-out all live in
// the engine, where PSK/APRS/SSTV already answer the same question once.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'

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
  rttySetAuto: vi.fn(),
  rttyAutoCq: vi.fn(),
  rttyAutoAnswer: vi.fn(),
  rttyAutoAbort: vi.fn(),
  atuTune: vi.fn(),
  setRfPower: vi.fn(),
  setTune: vi.fn(),
  haltTx: vi.fn(),
}))
// The log strip is stubbed: this suite is about the TX/keying wiring, and the real LogEntry
// reaches the logbook, the park directory and the callbook on mount. RttyCockpit.log.test.tsx
// renders it for real.
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyAutoArm = api.rttyAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const snap = {
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
  },
} as unknown as AppSnapshot

const IDLE = {
  armed: false,
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
} as unknown as RttyState
const ARMED = { ...IDLE, armed: true }

// One backing state, as the engine has: the auto-arm flips it and the 2 Hz poll reads it back.
// A fixed `mockResolvedValue` on the poll would overwrite the armed answer half a second later
// and the test would be measuring the mock, not the cockpit.
let state: RttyState = IDLE
beforeEach(() => {
  state = IDLE
  getRttyState.mockReset().mockImplementation(async () => state)
  rttyAutoArm.mockReset().mockImplementation(async () => {
    state = ARMED
    return state
  })
  getLicensedBandPlan.mockReset().mockResolvedValue([])
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('RTTY arms its decoder when the operator opens the view', () => {
  it('asks the engine to auto-arm on entry', async () => {
    render(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(rttyAutoArm).toHaveBeenCalledTimes(1))
  })

  it('renders the armed state the engine answers with', async () => {
    render(<RttyCockpit snap={snap} active />)
    // The Arm RX button flips from "Arm RX" to "RX armed" once the decoder is running.
    await waitFor(() => expect(screen.getByRole('button', { name: 'RX armed' })).toBeTruthy())
  })

  it('does NOT ask again while the operator stays on the view', async () => {
    const { rerender } = render(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(rttyAutoArm).toHaveBeenCalledTimes(1))
    rerender(<RttyCockpit snap={{ ...snap }} active />)
    rerender(<RttyCockpit snap={{ ...snap }} active />)
    expect(rttyAutoArm).toHaveBeenCalledTimes(1)
  })

  it('does not touch the decoder while RTTY is not the visible view', async () => {
    render(<RttyCockpit snap={snap} active={false} />)
    await Promise.resolve()
    expect(rttyAutoArm).not.toHaveBeenCalled()
  })

  it('asks again on the NEXT entry — the cockpit is kept alive, so mount fires once a session', async () => {
    // This is why the effect keys on the rising edge of `active` rather than on mount: the host
    // never unmounts this cockpit, so a mount-only arm would run once and never again.
    const { rerender } = render(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(rttyAutoArm).toHaveBeenCalledTimes(1))
    rerender(<RttyCockpit snap={snap} active={false} />) // navigate away
    rerender(<RttyCockpit snap={snap} active />) // and back
    await waitFor(() => expect(rttyAutoArm).toHaveBeenCalledTimes(2))
  })
})
