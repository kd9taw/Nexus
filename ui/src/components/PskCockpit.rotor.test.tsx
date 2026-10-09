// @vitest-environment jsdom
//
// THE ROTOR STRIP IN THE PSK COCKPIT: RttyCockpit.rotor.test.tsx's case, for the other keyboard
// cockpit (a tester asked for "the same rotor implementation in the other cockpits"). With the
// REAL strip: it sits in the header's own control cluster; → CALL and LP point at the settled
// call in the dock's Call box, the one the log strip takes; a station with no rotator sees
// nothing; a browser without the station's rotator control sees the unavailable plate; and the
// cockpit, which App keeps mounted behind every other screen, asks the rotator nothing while it is
// hidden.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { PskCockpit } from './PskCockpit'
import { pointRotatorAtCall, readRotator } from '../api'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, PskState } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "draws it in the header, and → CALL and LP point at the…", takes
// 0.81 s and 1.41 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const IDLE = {
  armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false,
  latched: false, keyerError: null,
} as unknown as PskState
/** The station's rotator: configured model and the azimuth it answers with (null = silent). */
const rotator: { model: number; az: number | null } = { model: 603, az: 212 }

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (CwCockpit.density's pattern).
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getPskState: vi.fn(async () => IDLE),
    pskAutoArm: vi.fn(async () => IDLE),
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
// Renders the header's children and nothing else: a strip passed to any other slot is not found.
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
  mygrid: 'EN61',
  hunt: null,
  radio: {
    dialMhz: 14.07, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: true, txAllowed: true,
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

describe('the PSK cockpit carries the rotor strip', () => {
  it('draws it in the header, and → CALL and LP point at the call in the Call box', async () => {
    render(<PskCockpit snap={snap} />)
    await waitFor(() => expect(strip(), 'no rotor strip in the PSK cockpit').not.toBeNull())
    expect(strip()!.closest('.cockpit-header'), 'the strip is not in the header').not.toBeNull()
    // CONTROL: nothing in the Call box yet, so there is nobody to beam at.
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()

    fireEvent.change(screen.getByLabelText(t('psk.hisCall.aria')), { target: { value: 'ja1abc' } })
    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }, { timeout: 2000 }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', false))
    fireEvent.click(screen.getByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', true))
  })

  it('shows nothing at all on a station with no rotator', async () => {
    rotator.model = 0
    rotator.az = null
    render(<PskCockpit snap={snap} />)
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
        <PskCockpit snap={snap} />
      </StationControlContext.Provider>,
    )
    const plate = await screen.findByLabelText(t('remote.rotatorUnavailable'))
    expect(plate.closest('.cockpit-header')).not.toBeNull()
    expect(strip()).toBeNull()
  })

  it('asks the rotator nothing while the cockpit is hidden', async () => {
    const r = render(<PskCockpit snap={snap} active={false} />)
    await settle()
    expect(poll, 'a hidden PSK cockpit polled the rotator').not.toHaveBeenCalled()
    // THE CONTROL: the same cockpit, shown, does ask.
    r.rerender(<PskCockpit snap={snap} active />)
    await waitFor(() => expect(poll).toHaveBeenCalled())
  })
})
