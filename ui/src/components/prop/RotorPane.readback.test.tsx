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
// So the two states are separated here: nothing configured draws only its one line (most stations),
// configured-but-silent keeps the controls and shows "—". Pinned because the honest-looking
// fix (a fake needle at 0°) and the tidy-looking one (hide it all) are both worse.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { t } from '../../i18n'
import type { RotatorState } from '../../types'

/** The DCU-1 case: rotctld answers, and the backend has no position to give, for ever. */
const DCU1: RotatorState = { azDeg: null, reading: 'noPosition' }

const api = vi.hoisted(() => ({
  // The browser's read (a hosted page keeps it); the desktop pane polls readRotatorState.
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  readRotatorState: vi.fn(
    (): Promise<{ azDeg: number | null; reading: string } | null> =>
      Promise.resolve({ azDeg: null, reading: 'noPosition' }),
  ),
  getMoonStatus: vi.fn(async () => null),
  startMoonTrack: vi.fn(async () => ({ state: 'following', elevation: true, azDeg: null, elDeg: null })),
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
  api.readRotatorState.mockImplementation(() => Promise.resolve(DCU1))
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
  it('draws no rose, only the line saying how to set one up', async () => {
    api.getSettings.mockImplementation(() =>
      Promise.resolve({ rotatorModel: 0, rotatorHost: '' } as never),
    )
    api.readRotatorState.mockImplementation(() => Promise.resolve(null))
    const { container } = render(<RotorPane />)
    // Give the settings read and the first poll a chance to land before concluding.
    await waitFor(() => expect(api.readRotatorState).toHaveBeenCalled())
    expect(container.querySelector('.rotor-pane')).toBeNull()
    expect(container.textContent).toBe(t('connect.pane.rotor.basic'))
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
    api.readRotatorState.mockImplementation(() => Promise.resolve({ azDeg: 213, reading: 'position' }))
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(/213°T/)).not.toBeNull())
    expect(container.querySelectorAll('.rotor-needle:not(.target)').length).toBe(1)
  })
})

describe('a controller that is switched off (a tester read "Error 61" on every command)', () => {
  it('says the controller is not answering, not that pointing still works', async () => {
    api.readRotatorState.mockImplementation(() => Promise.resolve({ azDeg: null, reading: 'notAnswering' }))
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.notAnswering'))).not.toBeNull())
    expect(screen.queryByText(t('rotor.pane.hint.noPosition'))).toBeNull()
    expect(screen.getByText(/—°T/).closest('[title]')?.getAttribute('title')).toBe(t('rotor.pane.notAnswering'))
    // …and STOP is still there: the pane never hides it.
    expect(screen.getByRole('button', { name: /stop/i })).not.toBeNull()
    expect(container.querySelectorAll('.rotor-needle:not(.target)').length).toBe(0)
  })

  it('one missed poll keeps the last bearing, dimmed, and only a run of misses says "not answering"', async () => {
    // ⭐ A rotator over the internet "lost connection" on every dropped reply: one missed poll
    // turned the pane to "isn't answering". Now it keeps the last reading, shown as stale.
    let n = 0
    api.readRotatorState.mockImplementation(() =>
      Promise.resolve(
        n++ === 0
          ? { azDeg: 213, reading: 'position', elDeg: 45, elRange: [0, 180] }
          : { azDeg: null, reading: 'notAnswering' },
      ),
    )
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(/213°T/)).not.toBeNull())
    // The second poll misses: the bearing stays, marked stale, and the pane does not cry wolf.
    await waitFor(() => expect(container.querySelector('.rotor-az.stale')).not.toBeNull(), { timeout: 4_000 })
    expect(screen.queryByText(/213°T/)).not.toBeNull()
    // …and on an az/el rotator (a G-5500) the elevation is kept the same way.
    expect(container.querySelector('.rotor-el.stale')?.textContent).toBe('EL 45°')
    expect(container.querySelector('.rotor-needle.stale')).not.toBeNull()
    expect(screen.queryByText(t('rotor.pane.stale'))).not.toBeNull()
    expect(screen.queryByText(t('rotor.pane.notAnswering'))).toBeNull()
    // Misses in a row: now it is not answering, and the needle goes.
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.notAnswering'))).not.toBeNull(), {
      timeout: 8_000,
    })
    expect(screen.queryByText(/213°T/)).toBeNull()
    expect(container.querySelectorAll('.rotor-needle:not(.target)').length).toBe(0)
  }, 15_000)

  it('the DCU-1, which answers with no position, keeps its own words', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.hint.noPosition'))).not.toBeNull())
    expect(screen.queryByText(t('rotor.pane.notAnswering'))).toBeNull()
  })
})
