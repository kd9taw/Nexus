// @vitest-environment jsdom
//
// The Rotor pane on an az/el rotator (the operator's Yaesu G-5500 on its GS-232B): the elevation
// beside the compass, a box to type one within the range the rotator's own backend declares, and
// the one ■ STOP for both axes. None of it appears on a rotator with no elevation axis, and none
// of it on a browser on Nexus Remote.
//
// What the backend does with these calls — the azimuth kept where the rotator reports it, the
// GS-232B's `W123 030` and its All Stop — is proven against Hamlib's real GS-232B backend in
// tempo-audio (`rotctld_gs232b.rs`); this file holds the pane to what it sends and shows.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, act } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { StationControlContext } from '../../stationAccess'
import { t } from '../../i18n'
import type { RotatorState } from '../../types'

/** A G-5500 on a GS-232B at 123° / 45°: what `read_rotator_state` answers for it. */
const G5500: RotatorState = { azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }
/** A Rotor-EZ: azimuth only, declared so. */
const ROTOR_EZ: RotatorState = { azDeg: 123, reading: 'position', elDeg: null, elRange: null }

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(123)),
  readRotatorState: vi.fn((): Promise<RotatorState | null> => Promise.resolve(null)),
  pointRotator: vi.fn((..._args: number[]) => Promise.resolve()),
  pointRotatorElevation: vi.fn((..._args: number[]) => Promise.resolve()),
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
  api.readRotator.mockImplementation(() => Promise.resolve(123))
  api.readRotatorState.mockImplementation(() => Promise.resolve(G5500))
  api.pointRotator.mockImplementation(() => Promise.resolve())
  api.pointRotatorElevation.mockImplementation(() => Promise.resolve())
  api.stopRotator.mockImplementation(() => Promise.resolve())
  api.getDeclination.mockImplementation(() => Promise.resolve(null))
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.stopSatTrack.mockImplementation(() => Promise.resolve())
  api.getSettings.mockImplementation(() =>
    Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never),
  )
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const elBox = () =>
  screen.queryByRole('spinbutton', { name: t('rotor.pane.el.entry.aria', { min: 0, max: 180 }) })
const bearingBox = () => screen.getByRole('spinbutton', { name: /azimuth to slew to/i })
const type = (box: HTMLElement, value: string) => {
  fireEvent.change(box, { target: { value } })
  fireEvent.keyDown(box, { key: 'Enter' })
}

describe('an az/el rotator (a G-5500 on its GS-232B)', () => {
  it('shows the elevation it reports beside the compass, and a box for a new one', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText('EL 45°')).not.toBeNull())
    expect(screen.getByText('EL 45°').getAttribute('title')).toBe(
      t('rotor.pane.el.title', { min: 0, max: 180 }),
    )
    const box = elBox()
    expect(box).not.toBeNull()
    // The range the backend declared, not one written into the pane.
    expect([box?.getAttribute('min'), box?.getAttribute('max')]).toEqual(['0', '180'])
    expect(screen.queryByText(t('rotor.pane.hint.azel'))).not.toBeNull()
  })

  it('a typed elevation goes out alone: the backend keeps the azimuth where the rotator is', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(elBox()).not.toBeNull())
    type(elBox()!, '30')
    await waitFor(() => expect(api.pointRotatorElevation).toHaveBeenCalledWith(30))
    // A satellite track is halted first, exactly as a bearing does, or its next tick undoes it.
    expect(api.stopSatTrack).toHaveBeenCalled()
    // …and shows where it was sent while the mast is on its way, as the bearing does.
    expect(screen.queryByText('→ EL 30°')).not.toBeNull()
    expect(api.pointRotator).not.toHaveBeenCalled()
  })

  it('an elevation the mount cannot reach is refused before anything is sent', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(elBox()).not.toBeNull())
    type(elBox()!, '181')
    await waitFor(() =>
      expect(toast.pushToast).toHaveBeenCalledWith(
        t('rotor.pane.el.outside', { min: 0, max: 180 }),
        'error',
      ),
    )
    expect(api.pointRotatorElevation).not.toHaveBeenCalled()
    expect(api.stopSatTrack).not.toHaveBeenCalled()
  })

  it('a bearing still on its way goes with the elevation typed after it, and STOP drops it', async () => {
    render(<RotorPane />)
    await waitFor(() => expect(elBox()).not.toBeNull())
    // Turn to 200°, then raise the antenna before the mast gets there: the elevation carries
    // the bearing, or the GS-232B's next W command would stop the turn where it stood.
    type(bearingBox(), '200')
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledWith(200))
    type(elBox()!, '30')
    await waitFor(() => expect(api.pointRotatorElevation).toHaveBeenCalledWith(30, 200))
    // A bearing typed while that elevation is still on its way carries it the same way.
    type(bearingBox(), '210')
    await waitFor(() => expect(api.pointRotator).toHaveBeenLastCalledWith(210, 30))

    // ONE STOP for both axes — and after it nothing is handed over: the next move keeps what the
    // rotator reports, so the mast is never pulled back to where it was sent before the stop.
    fireEvent.click(screen.getByRole('button', { name: /stop/i }))
    await waitFor(() => expect(api.stopRotator).toHaveBeenCalledTimes(1))
    type(elBox()!, '40')
    await waitFor(() => expect(api.pointRotatorElevation).toHaveBeenLastCalledWith(40))
  })

  it('keeps the elevation it knew when rotctld does not answer that question this time', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    render(<RotorPane />)
    await waitFor(() => expect(elBox()).not.toBeNull())
    // The next reading has the position but no declaration (a busy rotctld): no blink.
    api.readRotatorState.mockImplementation(() =>
      Promise.resolve({ azDeg: 124, reading: 'position', elDeg: 46 }),
    )
    await act(async () => {
      vi.advanceTimersByTime(2_100)
    })
    await waitFor(() => expect(screen.queryByText('EL 46°')).not.toBeNull())
    expect(elBox()).not.toBeNull()
    // A declaration that says NO elevation axis does take it away.
    api.readRotatorState.mockImplementation(() => Promise.resolve(ROTOR_EZ))
    await act(async () => {
      vi.advanceTimersByTime(2_100)
    })
    await waitFor(() => expect(elBox()).toBeNull())
    expect(screen.queryByText(/^EL /)).toBeNull()
  })
})

describe('a rotator with no elevation axis is unchanged', () => {
  it('shows no elevation and no elevation box', async () => {
    api.readRotatorState.mockImplementation(() => Promise.resolve(ROTOR_EZ))
    render(<RotorPane />)
    await waitFor(() => expect(screen.queryByText(/123°T/)).not.toBeNull())
    expect(elBox()).toBeNull()
    expect(screen.queryByText(/^EL /)).toBeNull()
    expect(screen.queryByText(t('rotor.pane.hint'))).not.toBeNull()
    // …and its bearing goes out exactly as it always did: the azimuth alone.
    type(bearingBox(), '200')
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledWith(200))
  })
})

describe('a browser on Nexus Remote', () => {
  it('keeps the azimuth-only pane it had', async () => {
    render(
      <StationControlContext.Provider value={false}>
        <RotorPane />
      </StationControlContext.Provider>,
    )
    await waitFor(() => expect(screen.queryByText(/123°T/)).not.toBeNull())
    expect(api.readRotatorState).not.toHaveBeenCalled()
    expect(elBox()).toBeNull()
    type(bearingBox(), '200')
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledWith(200))
  })
})
