// @vitest-environment jsdom
//
// THE ROTOR STRIP IN THE RTTY COCKPIT. A tester on 1.15.0: "the rotor activation from the phone
// cockpit works fine. Only thing I am missing is the same rotor implementation in the other
// cockpits; ft, rtty etc." Phone, CW and FT carried the shared `RotorStrip`; RTTY did not.
//
// What is pinned, with the REAL strip:
//   · it sits in the header's own control cluster, where CW puts it (the stub below renders the
//     header's children and nothing else, so a strip passed anywhere else is not found);
//   · → CALL and LP point at the call in the dock's Call box, settled, the same call the log
//     strip takes;
//   · a station with no rotator sees nothing, and a browser without the station's rotator control
//     sees the unavailable plate the other cockpits show;
//   · a HIDDEN RTTY cockpit asks the rotator nothing. This one is RTTY's own: App keeps this
//     cockpit mounted behind every other screen, so a strip polling from it would put
//     `read_rotator` on the engine lock every two seconds for as long as Nexus runs.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import { pointRotatorAtCall, readRotator } from '../api'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, RttyState } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "draws it in the header, and → CALL and LP point at the…", takes
// 0.81 s and 1.42 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const IDLE: RttyState = {
  armed: false, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170,
  backend: 'afsk', sending: false, latched: false, keyerError: null, markHz: 2125, spaceHz: 2295,
  auto: false, seqState: 'idle', peer: null, peerExchange: [], heardCq: null,
}
/** The station's rotator: configured model and the azimuth it answers with (null = silent). */
const rotator: { model: number; az: number | null } = { model: 603, az: 212 }

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern), so this file never
  // throws on mount over an export it did not list.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getRttyState: vi.fn(async () => IDLE),
    rttyAutoArm: vi.fn(async () => IDLE),
    getLicensedBandPlan: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ rotatorModel: rotator.model, rotatorHost: '' })),
    readRotator: vi.fn(async () => rotator.az),
    getDeclination: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    pointRotatorAtCall: vi.fn(async () => 47),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: unknown }) => (
    <header className="cockpit-header">{children as never}</header>
  ),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const point = vi.mocked(pointRotatorAtCall)
const poll = vi.mocked(readRotator)

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: true, tuning: false, txAllowed: true,
  },
} as unknown as AppSnapshot

const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
const strip = () => screen.queryByRole('group', { name: t('rotor.strip.aria') })

beforeEach(() => {
  rotator.model = 603
  rotator.az = 212
  point.mockClear()
  poll.mockClear()
})
afterEach(cleanup)

describe('the RTTY cockpit carries the rotor strip', () => {
  it('draws it in the header, and → CALL and LP point at the call in the Call box', async () => {
    render(<RttyCockpit snap={snap} />)
    await waitFor(() => expect(strip(), 'no rotor strip in the RTTY cockpit').not.toBeNull())
    expect(strip()!.closest('.cockpit-header'), 'the strip is not in the header').not.toBeNull()
    // CONTROL: nothing in the Call box yet, so there is nobody to beam at.
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()

    fireEvent.change(screen.getByLabelText(t('rtty.hisCall.aria')), { target: { value: 'ja1abc' } })
    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }, { timeout: 2000 }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', false))
    fireEvent.click(screen.getByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', true))
  })

  it('shows nothing at all on a station with no rotator', async () => {
    rotator.model = 0
    rotator.az = null
    render(<RttyCockpit snap={snap} />)
    // The strip is mounted and asking — so the empty header below is its answer, not its absence.
    await waitFor(() => expect(poll).toHaveBeenCalled())
    await settle()
    expect(strip()).toBeNull()
    expect(screen.queryByLabelText(t('remote.rotatorUnavailable'))).toBeNull()
    expect(document.querySelector('.cockpit-header')!.childElementCount).toBe(0)
  })

  it('shows a browser without the station’s rotator control the unavailable plate', async () => {
    render(
      <StationControlContext.Provider value={false}>
        <RttyCockpit snap={snap} />
      </StationControlContext.Provider>,
    )
    const plate = await screen.findByLabelText(t('remote.rotatorUnavailable'))
    expect(plate.closest('.cockpit-header')).not.toBeNull()
    expect(strip()).toBeNull()
  })

  it('asks the rotator nothing while the cockpit is hidden', async () => {
    const r = render(<RttyCockpit snap={snap} active={false} />)
    await settle()
    expect(poll, 'a hidden RTTY cockpit polled the rotator').not.toHaveBeenCalled()
    // THE CONTROL: the same cockpit, shown, does ask.
    r.rerender(<RttyCockpit snap={snap} active />)
    await waitFor(() => expect(poll).toHaveBeenCalled())
  })
})
