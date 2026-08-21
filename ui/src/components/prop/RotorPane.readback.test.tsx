// @vitest-environment jsdom
//
// A rotator you cannot READ is still a rotator you can POINT.
//
// The pane's whole control surface — the rose, click-to-slew, the typed bearing and the STOP
// button — hung off one line, `if (az == null) return null`. Readback failing is not the same
// as pointing failing, and for one curated model it is guaranteed: Hy-Gain DCU-1/DCU-1X
// (model 403) has no `get_position` in the bundled Hamlib at all, so it answers `p` with
// `RPRT -11` for ever while taking every `P` perfectly. Its owner got no compass, no slew and —
// the part that matters — no STOP.
//
// So the two states are separated here: nothing configured renders nothing (most stations),
// configured-but-silent keeps the controls and shows "—". Pinned because the honest-looking
// fix (a fake needle at 0°) and the tidy-looking one (hide it all) are both worse.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'

const api = vi.hoisted(() => ({
  // The DCU-1 case: `p` is refused, so the poll yields null for ever.
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  pointRotator: vi.fn(() => Promise.resolve()),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 403, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
}))
vi.mock('../../api', () => api)
vi.mock('../../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => {
  vi.clearAllMocks()
  api.readRotator.mockImplementation(() => Promise.resolve(null))
  api.pointRotator.mockImplementation(() => Promise.resolve())
  api.stopRotator.mockImplementation(() => Promise.resolve())
  api.getDeclination.mockImplementation(() => Promise.resolve(null))
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.stopSatTrack.mockImplementation(() => Promise.resolve())
  api.getSettings.mockImplementation(() =>
    Promise.resolve({ rotatorModel: 403, rotatorHost: '' } as never),
  )
})
afterEach(cleanup)

describe('a configured rotator that does not report its position', () => {
  it('keeps its STOP button and its slew controls', async () => {
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
    expect(screen.getByRole('button', { name: /stop/i })).not.toBeNull()
    expect(screen.getByRole('spinbutton', { name: /azimuth to slew to/i })).not.toBeNull()
  })

  it('says "—" rather than drawing a needle it does not have', async () => {
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
    expect(screen.getByText(/—°T/)).not.toBeNull()
    // A needle at 0° would be a lie about where the antenna is pointing.
    expect(container.querySelectorAll('.rotor-needle:not(.target)').length).toBe(0)
  })

  it('a typed bearing still reaches the rotator', async () => {
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
    const entry = screen.getByRole('spinbutton', { name: /azimuth to slew to/i })
    fireEvent.change(entry, { target: { value: '213' } })
    fireEvent.keyDown(entry, { key: 'Enter' })
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledWith(213))
  })

  it('…and so does STOP', async () => {
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
    fireEvent.click(screen.getByRole('button', { name: /stop/i }))
    await waitFor(() => expect(api.stopRotator).toHaveBeenCalled())
  })
})

describe('a station with no rotator at all', () => {
  it('renders nothing, so the pane frame can say how to set one up', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve({ rotatorModel: 0, rotatorHost: '' } as never),
    )
    const { container } = render(<RotorPane />)
    // Give the settings read and the first poll a chance to land before concluding.
    await waitFor(() => expect(api.readRotator).toHaveBeenCalled())
    expect(container.querySelector('.rotor-pane')).toBeNull()
  })

  it('an external rotctld address counts as configured', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve({ rotatorModel: 0, rotatorHost: '127.0.0.1:4533' } as never),
    )
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
  })
})

describe('a rotator that does answer is unchanged', () => {
  it('draws the needle and the true bearing', async () => {
    api.readRotator.mockImplementation(() => Promise.resolve(213))
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(/213°T/)).not.toBeNull())
    expect(container.querySelectorAll('.rotor-needle:not(.target)').length).toBe(1)
  })
})
