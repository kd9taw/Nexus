// @vitest-environment jsdom
//
// LOCK ON — the way back onto the bird after the dial gets away from you.
//
// Operator report: "should we introduce a button to lock on to the implied frequency if I move
// the dial on my own?" A knob move INSIDE the passband is already adopted as your position and
// the uplink mirrors it. What had no way back was leaving the passband — by hand, or because the
// rig came back somewhere else — after which the dial is somewhere the pass does not describe.
//
// The transponder cards could not fix it either: they are radio inputs, and clicking the one
// already selected fires no change event. The pick that would re-assert routing, band, mode and
// both legs was literally unreachable while it was the pick in force.
//
// What is pinned here:
//
//  - The button RE-RUNS THE HELD PICK — the same command, the same name, the same index. It is
//    not a second frequency-writing path: routing, the band, the commanded mode and the split
//    all come along, which they would not if this shoved a dial at the rig directly.
//  - It appears only where there is a pick to re-assert. A button that re-picked "whatever looks
//    right" would be choosing a transponder for the operator.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { SatellitesView } from './SatellitesView'
import type { SatDetail, SatTrackStatus, SatTransponderHeld } from '../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn(() => Promise.resolve(null)),
  getSatSchedule: vi.fn(() => Promise.resolve([])),
  getSatPassNeeds: vi.fn(() => Promise.resolve([])),
  getSatDetail: vi.fn(),
  getSettings: vi.fn(),
  setSettings: vi.fn(() => Promise.resolve({} as never)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<SatTransponderHeld | null> => Promise.resolve(null)),
  startSatTrack: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
}))
vi.mock('../api', () => api)
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)
const AOS = NOW - 300
const LOS = NOW + 300

/** Two transmitters, so the held INDEX has to be carried rather than assumed. */
const detail = (): SatDetail =>
  ({
    name: 'RS-44',
    norad: 44909,
    status: 'alive',
    transmitters: [
      { description: 'CW beacon', alive: true, kind: 'Transmitter', invert: false },
      { description: 'SSB/CW linear transponder', alive: true, kind: 'Transponder', invert: true },
    ],
    dataFetchedAt: 1_760_000_000,
    pass: {
      name: 'RS-44',
      aosUnix: AOS,
      losUnix: LOS,
      maxElDeg: 62,
      aosAzDeg: 100,
      losAzDeg: 260,
      status: 'alive',
    },
    passTrack: [
      [AOS, 100, 0],
      [NOW, 180, 62],
      [LOS, 260, 0],
    ],
  }) as unknown as SatDetail

const status = (over: Partial<SatTrackStatus> = {}): SatTrackStatus =>
  ({
    name: 'RS-44',
    state: 'tracking',
    mode: 'rotor+doppler',
    dopplerDownlink: true,
    dopplerUplink: true,
    uplinkOffer: 'none',
    uplinkOfferMap: null,
    uplinkRadio: 'IC-9700',
    uplinkRadioId: 1,
    azDeg: 141,
    elDeg: 46,
    aosAzDeg: 100,
    maxElDeg: 45,
    satAzDeg: 143,
    satElDeg: 47,
    rangeKm: 812,
    rangeRateKmS: -5.42,
    downlinkHz: 435_643_320,
    uplinkHz: 145_962_680,
    downlinkShiftHz: -2310,
    uplinkShiftHz: 770,
    transponder: 'SSB/CW linear transponder',
    transponderIndex: 1,
    inverting: true,
    offsetHz: 3200,
    halfWidthHz: 12_500,
    elementAgeDays: 1.2,
    elementEpochUnix: 1_785_442_400,
    aosUnix: AOS,
    losUnix: LOS,
    ...over,
  }) as SatTrackStatus

/** The engine's hold on transponder #1 — what the 2 s poll reads back. */
const held = (): SatTransponderHeld =>
  ({
    name: 'RS-44',
    index: 1,
    description: 'SSB/CW linear transponder',
    binding: null,
  }) as SatTransponderHeld

const settings = (over: Record<string, unknown> = {}) => ({
  mygrid: 'EN52',
  rotatorModel: 2,
  rotatorHost: '',
  satDopplerOff: false,
  satVfoMap: 'main-down-sub-up',
  ...over,
})

beforeEach(() => {
  localStorage.clear()
  api.getSatDetail.mockReset()
  api.getSatDetail.mockImplementation(() => Promise.resolve(detail()))
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() => Promise.resolve(settings()))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(status()))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(held()))
  api.setSatTransponder.mockClear()
})
afterEach(cleanup)

const lockOn = () => screen.findByRole('button', { name: /lock on/i })

describe('Lock on — putting the radio back on the bird', () => {
  it('re-runs the HELD pick, name and index intact', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await lockOn())

    // The same command a transponder card runs — which is the point. Anything
    // that wrote a frequency directly would skip routing, the band and the
    // commanded mode, and land the dial on a rig that was never asked.
    expect(api.setSatTransponder).toHaveBeenCalledTimes(1)
    expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 1)
  })

  it('sits with the frequencies it re-asserts, not somewhere else on the page', async () => {
    render(<SatellitesView focusSat="RS-44" />)
    const btn = await lockOn()
    // The Doppler readout is the block that PRINTS the numbers this button
    // puts the radio on; a control that means "go to these" belongs with them.
    expect(btn.closest('.sat-doppler')).toBeTruthy()
  })

  it('appears on the ENGINE’s hold, not just on a click this section saw', async () => {
    // THE OPERATOR REPORT, 0.27.1: "I don't see the button to lock on." He was
    // holding a transponder with Doppler correcting both legs — and this is
    // the ordinary shape of that, not an edge case. Arm a pass, open the
    // section afterwards (or come back to the bird), and the engine holds a
    // transponder while this section's own click-state never saw a click.
    //
    // The file's rule is engine truth first, local pick second — `heldT`
    // follows it and the readout's `held` prop is written to tolerate exactly
    // this. Asking the local pick alone made the button vanish precisely when
    // the operator was mid-pass, which is the only time it is any use.
    api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
    render(<SatellitesView focusSat="RS-44" />)
    fireEvent.click(await lockOn())
    // The index comes off the track DTO, which indexes the same getSatDetail
    // list the cards do — so it re-runs the pick the ENGINE is holding.
    expect(api.setSatTransponder).toHaveBeenCalledWith('RS-44', 1)
  })

  it('is absent when no transponder is held — it never picks one for you', async () => {
    api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
    // With nothing held the engine drives nothing, so the readout carries its
    // reason instead of frequencies.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        status({
          downlinkHz: null,
          uplinkHz: null,
          downlinkShiftHz: null,
          uplinkShiftHz: null,
          transponder: null,
          transponderIndex: null,
          dopplerDownlink: false,
          dopplerUplink: false,
        }),
      ),
    )
    render(<SatellitesView focusSat="RS-44" />)
    await screen.findByText(/No transponder selected/i)
    expect(screen.queryByRole('button', { name: /lock on/i })).toBeNull()
    expect(api.setSatTransponder).not.toHaveBeenCalled()
  })
})
