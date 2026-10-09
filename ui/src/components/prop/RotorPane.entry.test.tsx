// @vitest-environment jsdom
//
// THE ROTOR BOX'S LINE 2 BESIDE A COCKPIT: the call in that cockpit's log entry, the bearing and
// distance to it, and Point. The bearing is the station's answer for that call
// (`rotatorBearingToCall`, the point's own resolver read only; that it equals what Point turns to is
// proven by value in src-tauri, `the_bearing_the_box_shows_is_the_one_point_turns_the_antenna_to`),
// so this file holds the pane to drawing exactly that answer, for exactly the call in the entry, and
// to pointing at that call and nothing else.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { StationControlContext } from '../../stationAccess'
import { t } from '../../i18n'
import type { CallBearing, PointedAt, RotatorState } from '../../types'

vi.setConfig({ testTimeout: 15_000 })

/** EC1DD from JO21EV: its callbook grid IN52TK is at 227.4°, 1530.6 km. The centre of Spain, which a
 *  bearing worked out in the page might have used, is at 207° — so a 207 anywhere is the wrong one. */
const EC1DD: CallBearing = { pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }
/** AA1AA: its log form's grid FN42KH, 290.6° and 5379.2 km away. */
const AA1AA: CallBearing = { pointed: { bearing: 290.6, to: 'grid', grid: 'FN42KH', country: null }, km: 5379.2 }
const G5500: RotatorState = { azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }

// The api the pane may reach, and nothing else: a click that called anything outside this list
// (a transmit command, say) would throw here, at the access, rather than pass unseen.
const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(123)),
  readRotatorState: vi.fn((): Promise<RotatorState | null> => Promise.resolve(null)),
  pointRotator: vi.fn((..._args: number[]) => Promise.resolve()),
  pointRotatorElevation: vi.fn((..._args: number[]) => Promise.resolve()),
  pointRotatorAtCall: vi.fn((_call: string, _longPath?: boolean): Promise<PointedAt> => Promise.reject(new Error('unset'))),
  rotatorBearingToCall: vi.fn((_call: string): Promise<CallBearing> => Promise.reject(new Error('unset'))),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
}))
vi.mock('../../api', () => api)
vi.mock('../../toast', () => ({ pushToast: vi.fn() }))

const answers: Record<string, CallBearing> = { EC1DD, AA1AA }

beforeEach(() => {
  vi.clearAllMocks()
  // Metric unless a case says otherwise: jsdom's en-US locale would read 'auto' as miles.
  localStorage.setItem('nexus.units', 'metric')
  api.readRotator.mockImplementation(() => Promise.resolve(123))
  api.readRotatorState.mockImplementation(() => Promise.resolve(G5500))
  api.pointRotator.mockImplementation(() => Promise.resolve())
  api.pointRotatorElevation.mockImplementation(() => Promise.resolve())
  api.pointRotatorAtCall.mockImplementation((call: string) =>
    answers[call] ? Promise.resolve(answers[call].pointed) : Promise.reject(new Error('unknownStation')),
  )
  api.rotatorBearingToCall.mockImplementation((call: string) =>
    answers[call] ? Promise.resolve(answers[call]) : Promise.reject(new Error('unknownStation')),
  )
  api.stopRotator.mockImplementation(() => Promise.resolve())
  api.getDeclination.mockImplementation(() => Promise.resolve(null))
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.stopSatTrack.mockImplementation(() => Promise.resolve())
  api.getSettings.mockImplementation(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never))
})
afterEach(() => {
  cleanup()
  localStorage.removeItem('nexus.units')
})

const line2 = () => document.querySelector<HTMLElement>('.rotor-aim')
const pointButton = () => screen.queryByRole('button', { name: t('rotor.pane.aim.point.label') })
const shown = async () => {
  await waitFor(() => expect(screen.queryByText('123°T')).not.toBeNull())
}
const elBox = () => screen.getByRole('spinbutton', { name: t('rotor.pane.el.entry.aria', { min: 0, max: 180 }) })
const bearingBox = () => screen.getByRole('spinbutton', { name: /azimuth to slew to/i })
const type = (box: HTMLElement, value: string) => {
  fireEvent.change(box, { target: { value } })
  fireEvent.keyDown(box, { key: 'Enter' })
}

describe('line 2: the call in the log entry, with the bearing and distance the station answers for it', () => {
  it('reads "→ CALL 227° (1531 km)" off the station\'s answer for that call', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line2()?.textContent).toContain('227°'))
    expect(api.rotatorBearingToCall).toHaveBeenCalledWith('EC1DD')
    expect(line2()!.querySelector('.rotor-aim-to')!.textContent).toBe('→ EC1DD 227° (1531 km)')
    expect(line2()!.getAttribute('title')).toBe(
      t('rotor.pane.aim.title', { call: 'EC1DD', to: t('rotor.pointed.to.grid', { grid: 'IN52TK' }) }),
    )
    // Not the centre of Spain, which is what a bearing worked out here from the country would say.
    expect(line2()!.textContent).not.toContain('207')
    expect(pointButton()).not.toBeNull()
  })

  it('gives the distance in the operator\'s unit', async () => {
    localStorage.setItem('nexus.units', 'imperial')
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line2()?.querySelector('.rotor-aim-to')?.textContent).toBe('→ EC1DD 227° (951 mi)'))
  })

  it('follows the entry: a new call never shows the last call\'s bearing', async () => {
    let answer: (b: CallBearing) => void = () => {}
    const { rerender } = render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line2()?.textContent).toContain('227°'))
    api.rotatorBearingToCall.mockImplementation((call: string) =>
      call === 'AA1AA' ? new Promise<CallBearing>((r) => (answer = r)) : Promise.resolve(answers[call]),
    )
    rerender(<RotorPane entryCall="AA1AA" />)
    await waitFor(() => expect(api.rotatorBearingToCall).toHaveBeenLastCalledWith('AA1AA'))
    // Asked, not yet answered: the call alone, never EC1DD's 227°.
    expect(line2()!.querySelector('.rotor-aim-to')!.textContent).toBe('→ AA1AA')
    answer(AA1AA)
    await waitFor(() => expect(line2()?.querySelector('.rotor-aim-to')?.textContent).toBe('→ AA1AA 291° (5379 km)'))
  })
})

describe('no call, no line; no bearing, the honest words', () => {
  it('an empty entry draws no line 2 and asks the station nothing', async () => {
    const { rerender } = render(<RotorPane />)
    await shown()
    rerender(<RotorPane entryCall={null} />)
    rerender(<RotorPane entryCall="" />)
    await shown()
    expect(line2()).toBeNull()
    expect(pointButton()).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })

  it('a station nothing places says so in words, with no bearing and no Point', async () => {
    render(<RotorPane entryCall="QQ1QQ" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.aim.unknown'))).not.toBeNull())
    expect(line2()!.querySelector('.rotor-aim-to')!.textContent).toBe('→ QQ1QQ')
    expect(line2()!.textContent).not.toMatch(/\d°/)
    expect(pointButton()).toBeNull()
  })

  it('no grid of your own says where to set it, with no bearing and no Point', async () => {
    api.rotatorBearingToCall.mockImplementation(() => Promise.reject('noGrid'))
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.aim.noGrid'))).not.toBeNull())
    expect(line2()!.textContent).not.toMatch(/\d°/)
    expect(pointButton()).toBeNull()
  })
})

describe('Point', () => {
  it('turns the antenna to the ENTRY\'s call, short path, and does nothing else', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line2()?.textContent).toContain('227°'))
    fireEvent.click(pointButton()!)
    await waitFor(() => expect(api.pointRotatorAtCall).toHaveBeenCalledTimes(1))
    expect(api.pointRotatorAtCall).toHaveBeenCalledWith('EC1DD', false)
    // The point-at-call alone: no slew of the pane's own, no stop, no satellite-track stop.
    expect(api.pointRotator).not.toHaveBeenCalled()
    expect(api.pointRotatorElevation).not.toHaveBeenCalled()
    expect(api.stopRotator).not.toHaveBeenCalled()
    expect(api.stopSatTrack).not.toHaveBeenCalled()
  })

  it('hands no earlier target of this pane on: the next move cannot pull the mast back', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line2()?.textContent).toContain('227°'))
    // Turn to 90° from the pane, then Point before the mast gets there…
    type(bearingBox(), '90')
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledWith(90))
    await waitFor(() => expect(screen.queryByText('→ 90°')).not.toBeNull())
    fireEvent.click(pointButton()!)
    await waitFor(() => expect(api.pointRotatorAtCall).toHaveBeenCalledWith('EC1DD', false))
    // …and the pane's own 90° is gone from the screen and from the next move: an elevation typed now
    // goes out alone, so the backend keeps the azimuth where the rotator is, on its way to EC1DD.
    await waitFor(() => expect(screen.queryByText('→ 90°')).toBeNull())
    type(elBox(), '30')
    await waitFor(() => expect(api.pointRotatorElevation).toHaveBeenCalledTimes(1))
    expect(api.pointRotatorElevation).toHaveBeenCalledWith(30)
  })
})

describe('a seat without station control', () => {
  it('gets no line 2, no live Point, and asks the station for no bearing', async () => {
    render(
      <StationControlContext.Provider value={false}>
        <RotorPane entryCall="EC1DD" />
      </StationControlContext.Provider>,
    )
    // The pane is there, with the browser's own read of the heading (the control that it rendered).
    await waitFor(() => expect(screen.queryByText('123°T')).not.toBeNull())
    expect(line2()).toBeNull()
    expect(pointButton()).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })
})
