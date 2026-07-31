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
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const NOW = Math.floor(Date.now() / 1000)

const track = (over: Partial<SatTrackStatus> = {}): SatTrackStatus => ({
  name: 'RS-44',
  state: 'tracking',
  mode: 'doppler-only',
  azDeg: null,
  elDeg: null,
  aosAzDeg: 100,
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
  aosUnix: NOW - 300,
  losUnix: NOW + 300,
  ...over,
})

beforeEach(() => {
  api.readRotator.mockReset()
  api.readRotator.mockImplementation(() => Promise.reject(new Error('no rotor')))
  api.getSatTrackStatus.mockReset()
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
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
})
