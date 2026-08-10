// @vitest-environment jsdom
//
// Doppler dial ownership OUTSIDE the Satellites section (litigation top-5 ④).
//
// A frequency moving by itself with no visible owner is the worst trust failure in
// this app. The strip used to render NOTHING for a rotor-less station — which is
// exactly the station whose dial a rotor-less Doppler track now steers. Pinned:
//
//  - No rotor + a Doppler-driving sat track ⇒ an ownership chip names the bird and
//    the dial, and its ■ stops the TRACK (the loop would redo a bare rotor stop).
//  - No rotor + a pass-only track ⇒ NOTHING. A pass-only track moves nothing —
//    claiming ownership it does not have would be the opposite failure.
//  - No rotor + no track ⇒ nothing, unchanged (most stations).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorStrip } from './RotorStrip'
import type { SatTrackStatus } from '../types'

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.reject(new Error('no rotor'))),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() =>
    Promise.resolve({ rotatorModel: 0, rotatorHost: '' } as never),
  ),
  getSatTrackStatus: vi.fn((): Promise<SatTrackStatus | null> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn(
    (): Promise<import('../types').SatTransponderHeld | null> => Promise.resolve(null),
  ),
  setSatTransponder: vi.fn(() => Promise.resolve()),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

const track = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'doppler-only',
  dopplerDownlink: true,
  dopplerUplink: true,
  uplinkOffer: 'none',
  uplinkOfferMap: null,
  uplinkRadio: 'IC-9700',
  uplinkRadioId: 1,
  azDeg: null,
  elDeg: null,
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
  transponderIndex: 2,
  inverting: true,
  offsetHz: 3200,
  halfWidthHz: 12_500,
  elementAgeDays: 1.2,
  elementEpochUnix: 1_785_442_400,
  aosUnix: NOW - 300,
  losUnix: NOW + 300,
  ...over,
})

beforeEach(() => {
  api.readRotator.mockReset()
  api.readRotator.mockImplementation(() => Promise.reject(new Error('no rotor')))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.getSatTransponder.mockReset()
  api.getSatTransponder.mockImplementation(() => Promise.resolve(null))
  api.setSatTransponder.mockClear()
  // Rotor-less by default; the "configured but silent" cases opt in.
  api.getSettings.mockReset()
  api.getSettings.mockImplementation(() =>
    Promise.resolve({ rotatorModel: 0, rotatorHost: '' } as never),
  )
  api.stopSatTrack.mockClear()
})
afterEach(cleanup)

describe('the dial-ownership marker (no rotor configured)', () => {
  it('names the bird and the dial while a Doppler track steers the radio', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(track()))
    render(<RotorStrip />)
    const chip = await screen.findByRole('group', { name: /doppler owns the dial/i })
    expect(chip.textContent).toMatch(/RS-44/)
    expect(chip.textContent).toMatch(/dial/i)
  })

  it('its ■ stops the TRACK, not a single slew', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(track()))
    render(<RotorStrip />)
    await screen.findByRole('group', { name: /doppler owns the dial/i })
    fireEvent.click(screen.getByRole('button', { name: /stop/i }))
    await waitFor(() => expect(api.stopSatTrack).toHaveBeenCalled())
  })

  it('claims NOTHING for a pass-only track — nothing is being driven', async () => {
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(track({ mode: 'pass-only', downlinkHz: null, uplinkHz: null })),
    )
    const { container } = render(<RotorStrip />)
    // Give the poll a beat, then hold: still empty.
    await waitFor(() => expect(api.getSatTrackStatus).toHaveBeenCalled())
    expect(container.textContent).toBe('')
  })

  it('renders nothing with no rotor and no track (most stations, unchanged)', async () => {
    const { container } = render(<RotorStrip />)
    await waitFor(() => expect(api.getSatTrackStatus).toHaveBeenCalled())
    expect(container.textContent).toBe('')
  })

  it('a HELD transponder with no armed track shows the bird holding the dial (sat-FT batch)', async () => {
    // The QO-100/pre-AOS case: the pick parked the dial on the downlink and —
    // since the sat-FT batch — section entry and tier flips stand down for it.
    // A dial that will not re-home with no visible owner is the same trust
    // failure the track chip exists for, so the hold gets its own chip, and
    // its ■ releases the HOLD (there is no track to stop).
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({ name: 'QO-100', index: 0, description: 'NB transponder', binding: null }),
    )
    render(<RotorStrip />)
    const chip = await screen.findByRole('group', { name: /transponder holds the dial/i })
    expect(chip.textContent).toMatch(/QO-100/)
    expect(chip.textContent).toMatch(/holds the dial/i)
    fireEvent.click(screen.getByRole('button', { name: /release/i }))
    await waitFor(() => expect(api.setSatTransponder).toHaveBeenCalledWith('QO-100', null))
    expect(api.stopSatTrack).not.toHaveBeenCalled()
  })

  it('a live Doppler track outranks the held chip — one owner named at a time', async () => {
    api.getSatTrackStatus.mockImplementation(() => Promise.resolve(track()))
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({ name: 'RS-44', index: 2, description: 'linear', binding: null }),
    )
    render(<RotorStrip />)
    await screen.findByRole('group', { name: /doppler owns the dial/i })
    expect(screen.queryByRole('group', { name: /transponder holds the dial/i })).toBeNull()
  })

  it('never claims the dial for an uplink-only track — it names the TX VFO (round 3)', async () => {
    // Defect 5: dial ownership keys on the DOWNLINK leg. Under uplink-only
    // confirmed-and-driving the engine writes only the split TX VFO; the chip
    // stays visible (a frequency is still moving by itself) but must name the
    // VFO it actually owns, not the dial it never touched.
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(
        track({ dopplerDownlink: false, dopplerUplink: true, downlinkHz: null }),
      ),
    )
    render(<RotorStrip />)
    const chip = await screen.findByRole('group', { name: /doppler owns the tx vfo/i })
    expect(chip.textContent).toMatch(/RS-44/)
    expect(chip.textContent).not.toMatch(/holds the dial/i)
    expect(chip.textContent).toMatch(/TX VFO/)
    expect(screen.queryByRole('group', { name: /doppler owns the dial/i })).toBeNull()
  })
})

// ⭐ THE STATE THE ROTOR GIVE-UP FIX CREATES, and the one this strip used to go
// blind in. A rotator that stopped answering is, by definition, CONFIGURED and
// silent — and the ownership chip lived only in the "no rotator configured"
// branch, so it fell through to the dim placeholder, which carries no track
// information at all. The track is still alive (that is the whole point of the
// fix: it keeps the dial and runs Doppler to LOS), so the operator kept a
// frequency moving by itself and lost both the app-wide sign of who owned it
// and the ■ that stops it.
describe('a rotator configured but not answering', () => {
  const configured = () =>
    api.getSettings.mockImplementation(() =>
      Promise.resolve({ rotatorModel: 2, rotatorHost: '' } as never),
    )

  it('still names the bird and the dial, and still offers the ■', async () => {
    configured()
    api.getSatTrackStatus.mockImplementation(() =>
      Promise.resolve(track({ mode: 'doppler-only', rotorLost: true })),
    )
    render(<RotorStrip />)
    const chip = await screen.findByRole('group', { name: /doppler owns the dial/i })
    expect(chip.textContent).toMatch(/RS-44/)
    expect(chip.textContent).toMatch(/holds the dial/i)
    // The honest "no answer from the mast" placeholder is still there beside it.
    expect(screen.getByLabelText(/rotator stopped answering/i)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /stop the satellite track/i }))
    await waitFor(() => expect(api.stopSatTrack).toHaveBeenCalled())
  })

  it('claims nothing when no track is driving — just the honest placeholder', async () => {
    configured()
    render(<RotorStrip />)
    const dim = await screen.findByLabelText(/rotator not answering/i)
    expect(dim.textContent).toMatch(/ROTOR/)
    expect(screen.queryByRole('group', { name: /doppler owns/i })).toBeNull()
  })
})
