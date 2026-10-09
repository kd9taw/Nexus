// @vitest-environment jsdom
//
// THE ROTOR STRIP IN THE JS8 COCKPIT (a tester asked for "the same rotor implementation in the
// other cockpits"). JS8's station in play is the SELECTED one: the heard station the log strip is
// prefilled from. → CALL points there, and not at whatever the To box holds, because that box
// also takes group addresses (@ALLCALL) and calls nobody has heard. With the REAL strip: it sits
// in the header's own control cluster; a station with no rotator sees nothing; a browser without
// the station's rotator control sees the unavailable plate; and the cockpit, which App keeps
// mounted behind every other screen, asks the rotator nothing while it is hidden.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import { pointRotatorAtCall, readRotator } from '../api'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, Js8State } from '../types'
import type { PanelLayoutApi, Js8PanelId } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "draws it in the header, and → CALL and LP point at the…", takes
// 0.43 s and 0.35 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const JS8: Js8State = {
  speed: 'normal', rxSpeeds: 15, txEnabled: false, sending: false, hbOn: false, hbNextAtMs: null,
  hbIntervalMin: 0, cqOn: false, cqNextAtMs: null, cqIntervalMin: 0, autoreply: true, relay: true,
  hbAck: false, armed: { autoreply: false, cq: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0, idleLimitMin: 60, idleTripped: false, activity: [],
  stations: [
    { call: 'W0IND', grid: 'EN52', snrDb: -8, freqHz: 1508, speed: 'normal', lastMs: 1_757_000_015_000, lastHb: false, lastCq: true, storedMsgs: 0 },
  ],
  inbox: [], queue: [], pendingReply: null, lastError: null,
}
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
    getJs8State: vi.fn(async () => JS8),
    js8Enter: vi.fn(async () => JS8),
    // The logbook never answers: the roster's joined columns are not what this file is about.
    askLog: vi.fn(() => new Promise(() => {})),
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
  radio: {
    dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5,
  },
} as unknown as AppSnapshot

function fakePanels(): PanelLayoutApi<Js8PanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
const strip = () => screen.queryByRole('group', { name: t('rotor.strip.aria') })
const toBox = () => screen.getByLabelText(t('js8.dock.to.aria')) as HTMLInputElement

beforeEach(() => {
  rotator.model = 603
  rotator.az = 212
  point.mockClear()
  poll.mockClear()
  window.localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the JS8 cockpit carries the rotor strip', () => {
  it('draws it in the header, and → CALL and LP point at the selected station', async () => {
    render(<Js8Cockpit snap={snap} panels={fakePanels()} />)
    await waitFor(() => expect(strip(), 'no rotor strip in the JS8 cockpit').not.toBeNull())
    expect(strip()!.closest('.cockpit-header'), 'the strip is not in the header').not.toBeNull()
    // CONTROL: nobody selected yet, so there is nobody to beam at.
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()

    const row = await waitFor(() => {
      const b = [...document.querySelectorAll<HTMLButtonElement>('button.js8-station-call')].find((e) => e.textContent === 'W0IND')
      expect(b, 'the heard station is not in the roster').toBeTruthy()
      return b!
    })
    fireEvent.click(row)
    fireEvent.click(await screen.findByRole('button', { name: '→ W0IND' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('W0IND', false))
    fireEvent.click(screen.getByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('W0IND', true))
  })

  it('offers no beam at a group address or a call nobody has heard', async () => {
    render(<Js8Cockpit snap={snap} panels={fakePanels()} />)
    await waitFor(() => expect(strip()).not.toBeNull())
    fireEvent.change(toBox(), { target: { value: '@ALLCALL' } })
    await settle()
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()
    fireEvent.change(toBox(), { target: { value: 'K1XYZ' } })
    await settle()
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()
    // …and the heard one, typed rather than clicked, is the station in play.
    fireEvent.change(toBox(), { target: { value: 'w0ind' } })
    expect(await screen.findByRole('button', { name: '→ W0IND' })).toBeTruthy()
  })

  it('shows nothing at all on a station with no rotator', async () => {
    rotator.model = 0
    rotator.az = null
    render(<Js8Cockpit snap={snap} panels={fakePanels()} />)
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
        <Js8Cockpit snap={snap} panels={fakePanels()} />
      </StationControlContext.Provider>,
    )
    const plate = await screen.findByLabelText(t('remote.rotatorUnavailable'))
    expect(plate.closest('.cockpit-header')).not.toBeNull()
    expect(strip()).toBeNull()
  })

  it('asks the rotator nothing while the cockpit is hidden', async () => {
    const r = render(<Js8Cockpit snap={snap} panels={fakePanels()} active={false} />)
    await settle()
    expect(poll, 'a hidden JS8 cockpit polled the rotator').not.toHaveBeenCalled()
    // THE CONTROL: the same cockpit, shown, does ask.
    r.rerender(<Js8Cockpit snap={snap} panels={fakePanels()} active />)
    await waitFor(() => expect(poll).toHaveBeenCalled())
  })
})
