// @vitest-environment jsdom
//
// FOLLOW THE MOON (EME) on the Rotor pane: the Moon's position from the station, the one button that
// starts the follow, the state it reports, the one Stop it answers to (the same stopSatTrack a pass
// does), and the EME window with line 2's station. The pointing itself is the backend's
// (`run_moon_track`, tested in src-tauri against a rotctld socket).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { t } from '../../i18n'
import type { CallBearing, MoonStatus, MoonTrack, RotatorState } from '../../types'

vi.setConfig({ testTimeout: 15_000 })

const G5500: RotatorState = { azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }
const UP: MoonStatus = { azDeg: 104.7, elDeg: 26.4, track: null, window: null }
const FOLLOWING: MoonTrack = { state: 'following', elevation: true, azDeg: 104.7, elDeg: 26.4 }

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(123)),
  readRotatorState: vi.fn((): Promise<RotatorState | null> => Promise.resolve(null)),
  getMoonStatus: vi.fn((_dx?: string | null): Promise<MoonStatus | null> => Promise.resolve(null)),
  startMoonTrack: vi.fn((): Promise<MoonTrack> => Promise.reject(new Error('unset'))),
  pointRotator: vi.fn((..._args: number[]) => Promise.resolve()),
  pointRotatorElevation: vi.fn((..._args: number[]) => Promise.resolve()),
  rotatorBearingToCall: vi.fn((_call: string): Promise<CallBearing> => Promise.reject(new Error('unknownStation'))),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
}))
vi.mock('../../api', () => api)
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../../toast', () => toast)

beforeEach(() => {
  vi.clearAllMocks()
  api.readRotatorState.mockImplementation(() => Promise.resolve(G5500))
  api.getMoonStatus.mockImplementation(() => Promise.resolve(UP))
  api.startMoonTrack.mockImplementation(() => Promise.resolve(FOLLOWING))
  api.stopSatTrack.mockImplementation(() => Promise.resolve())
  api.getSettings.mockImplementation(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never))
})
afterEach(cleanup)

const moonLine = () => document.querySelector('.rotor-moon')

describe('the Moon on the Rotor pane', () => {
  it('says where the Moon is, and offers to follow it', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(moonLine()).not.toBeNull())
    expect(moonLine()!.textContent).toContain(t('rotor.pane.moon.at', { az: 105, el: 26 }))
    expect(screen.getByRole('button', { name: t('rotor.pane.moon.follow') })).not.toBeNull()
  })

  it('no grid, no Moon line: where it stands depends on where you are', async () => {
    api.getMoonStatus.mockImplementation(() => Promise.resolve(null))
    const { container } = render(<RotorPane />)
    await waitFor(() => expect(container.querySelector('.rotor-pane')).not.toBeNull())
    await waitFor(() => expect(api.getMoonStatus).toHaveBeenCalled())
    expect(moonLine()).toBeNull()
  })

  it('Follow starts the follow, says so, and Stop following is the one Stop every tracker answers to', async () => {
    render(<RotorPane />)
    fireEvent.click(await screen.findByRole('button', { name: t('rotor.pane.moon.follow') }))
    expect(api.startMoonTrack).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.moon.following'))).not.toBeNull())
    api.getMoonStatus.mockImplementation(() => Promise.resolve({ ...UP, track: FOLLOWING }))
    fireEvent.click(screen.getByRole('button', { name: t('rotor.pane.moon.stop') }))
    await waitFor(() => expect(api.stopSatTrack).toHaveBeenCalled())
    expect(api.stopRotator).not.toHaveBeenCalled()
  })

  it('says why it holds: the Moon below the horizon, an azimuth-only rotator, a rotator that stopped answering', async () => {
    const cases: [MoonTrack, string][] = [
      [{ ...FOLLOWING, state: 'belowHorizon' }, t('rotor.pane.moon.below')],
      [{ ...FOLLOWING, elevation: false, elDeg: null }, t('rotor.pane.moon.following.azOnly')],
      [{ ...FOLLOWING, state: 'rotorLost' }, t('rotor.pane.moon.lost')],
    ]
    for (const [track, words] of cases) {
      api.getMoonStatus.mockImplementation(() => Promise.resolve({ ...UP, elDeg: -5, track }))
      render(<RotorPane />)
      await waitFor(() => expect(screen.queryByText(words)).not.toBeNull())
      cleanup()
    }
  })

  it('a follow that cannot start says why', async () => {
    api.startMoonTrack.mockImplementation(() => Promise.reject(new Error('no grid')))
    render(<RotorPane />)
    fireEvent.click(await screen.findByRole('button', { name: t('rotor.pane.moon.follow') }))
    await waitFor(() =>
      expect(toast.pushToast).toHaveBeenCalledWith(t('rotor.pane.moon.failed', { error: 'no grid' }), 'error'),
    )
  })

  it("beside a cockpit, gives the EME window with the call's station, and asks about its grid only", async () => {
    api.rotatorBearingToCall.mockImplementation(() =>
      Promise.resolve({ pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }),
    )
    const start = Date.UTC(2026, 10, 3, 9, 10) / 1000
    const end = Date.UTC(2026, 10, 3, 13, 40) / 1000
    api.getMoonStatus.mockImplementation((dx?: string | null) =>
      Promise.resolve({ ...UP, window: dx === 'IN52TK' ? { start, end, openNow: false } : null }),
    )
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(
      () =>
        expect(
          screen.queryByText(t('rotor.pane.moon.window.next', { call: 'EC1DD', start: '09:10', end: '13:40' })),
        ).not.toBeNull(),
      { timeout: 6_000 },
    )
  })

  it('a station placed only by its country gets no window: a country is not a place to aim at the Moon from', async () => {
    api.rotatorBearingToCall.mockImplementation(() =>
      Promise.resolve({ pointed: { bearing: 227.4, to: 'country', grid: null, country: 'Spain' }, km: 1530.6 }),
    )
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(document.querySelector('.rotor-aim')).not.toBeNull())
    await waitFor(() => expect(api.getMoonStatus.mock.calls.length).toBeGreaterThan(1), { timeout: 6_000 })
    expect(api.getMoonStatus.mock.calls.every((c) => c[0] == null)).toBe(true)
    expect(document.querySelector('.rotor-moon-window')).toBeNull()
  })
})
