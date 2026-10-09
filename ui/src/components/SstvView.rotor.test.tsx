// @vitest-environment jsdom
//
// THE ROTOR STRIP IN THE SSTV COCKPIT (a tester asked for "the same rotor implementation in the
// other cockpits"). SSTV has no log strip and no Call box. The station it answers is the one the
// Reply preset uses: the newest FSK ID the decoder heard. So that is where → CALL points. With the
// REAL strip: it sits in the header's own control cluster, after SSTV's receive controls; a
// station with no rotator sees nothing of it; a browser without the station's rotator control sees
// the unavailable plate; and the cockpit, which App keeps mounted behind every other screen (its
// VIS receiver never sleeps), asks the rotator nothing while it is hidden.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { SstvView } from './SstvView'
import { pointRotatorAtCall, readRotator } from '../api'
import { StationControlContext } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "draws it in the header, and → CALL and LP point at the…", takes
// 0.32 s and 0.31 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NO_HEALTH: SstvHealth = {
  armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null,
  unknownVis: 0, lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null,
}
const IDLE: SstvState = {
  armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0,
  previewHeight: 0, hedrShiftHz: 0, gallery: [], health: NO_HEALTH, sending: false, txMode: null,
  txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0,
}
/** A picture that arrived with its sender's FSK ID: the station SSTV would reply to. */
const HEARD: SstvState = {
  ...IDLE,
  gallery: [
    { path: '/tmp/x.png', mode: 'Scottie 1', finishedUtc: '2026-08-16T12:00:00Z', freqMhz: 14.23, lines: 256, fskId: 'JA1ABC' },
  ],
}
const sstv: { current: SstvState } = { current: HEARD }
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
    getSstvState: vi.fn(async () => sstv.current),
    sstvAutoArm: vi.fn(async () => sstv.current),
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
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="band-waterfall" /> }))

const point = vi.mocked(pointRotatorAtCall)
const poll = vi.mocked(readRotator)

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.23, band: '20m', catOk: true, sideband: 'USB', transmitting: false,
    txEnabled: true, tuning: false, txAllowed: true,
  },
} as unknown as AppSnapshot

const settle = () => act(async () => { for (let i = 0; i < 8; i++) await Promise.resolve() })
const strip = () => screen.queryByRole('group', { name: t('rotor.strip.aria') })

beforeEach(() => {
  sstv.current = HEARD
  rotator.model = 603
  rotator.az = 212
  point.mockClear()
  poll.mockClear()
})
afterEach(cleanup)

describe('the SSTV cockpit carries the rotor strip', () => {
  it('draws it in the header, and → CALL and LP point at the station last heard', async () => {
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(strip(), 'no rotor strip in the SSTV cockpit').not.toBeNull())
    expect(strip()!.closest('.cockpit-header'), 'the strip is not in the header').not.toBeNull()

    fireEvent.click(await screen.findByRole('button', { name: '→ JA1ABC' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', false))
    fireEvent.click(screen.getByRole('button', { name: 'LP' }))
    await waitFor(() => expect(point).toHaveBeenCalledWith('JA1ABC', true))
  })

  it('offers no beam before a station has been heard', async () => {
    sstv.current = IDLE
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(strip()).not.toBeNull())
    await settle()
    expect(screen.queryByRole('button', { name: /^→/ })).toBeNull()
    expect(screen.queryByRole('button', { name: 'LP' })).toBeNull()
  })

  it('shows nothing of it on a station with no rotator', async () => {
    rotator.model = 0
    rotator.az = null
    render(<SstvView snap={snap} />)
    // The strip is mounted and asking — so the absence below is its answer, not a missing strip.
    await waitFor(() => expect(poll).toHaveBeenCalled())
    await settle()
    expect(strip()).toBeNull()
    expect(screen.queryByLabelText(t('remote.rotatorUnavailable'))).toBeNull()
  })

  it('shows a browser without the station’s rotator control the unavailable plate', async () => {
    render(
      <StationControlContext.Provider value={false}>
        <SstvView snap={snap} />
      </StationControlContext.Provider>,
    )
    const plate = await screen.findByLabelText(t('remote.rotatorUnavailable'))
    expect(plate.closest('.cockpit-header')).not.toBeNull()
    expect(strip()).toBeNull()
  })

  it('asks the rotator nothing while the cockpit is hidden', async () => {
    const r = render(<SstvView snap={snap} active={false} />)
    await settle()
    expect(poll, 'a hidden SSTV cockpit polled the rotator').not.toHaveBeenCalled()
    // THE CONTROL: the same cockpit, shown, does ask.
    r.rerender(<SstvView snap={snap} active />)
    await waitFor(() => expect(poll).toHaveBeenCalled())
  })
})
