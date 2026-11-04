// @vitest-environment jsdom
//
// LONG-PATH BEAM ON THE ROTOR PANE (#338). The cockpit header strip has had the pair since #338 —
// → CALL for the short path and LP for the reciprocal (RotorStrip.longpath.test.tsx) — and the
// pane's line 2 had Point alone. This is the same pair, by the same rules: LP asks the station for
// the reciprocal (`pointRotatorAtCall(call, true)`, through the cockpits' own `rotorPointAt`), the
// short-path Point still asks for short, and with no station in play there is neither. Two buttons,
// never a toggle, for the reason the strip gives: every bearing Nexus shows is short path, and a
// state the operator can misread points the beam 180° wrong.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { t } from '../../i18n'
import type { CallBearing, PointedAt, RotatorState } from '../../types'

vi.setConfig({ testTimeout: 15_000 })

const DL1ABC: CallBearing = { pointed: { bearing: 52.0, to: 'grid', grid: 'JO62QM', country: null }, km: 7000 }
const POINTED: PointedAt = { bearing: 52.0, to: 'grid', grid: 'JO62QM', country: null }

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<number | null> => Promise.resolve(212)),
  readRotatorState: vi.fn(
    (): Promise<RotatorState | null> => Promise.resolve({ azDeg: 212, reading: 'position', elDeg: null, elRange: null }),
  ),
  getMoonStatus: vi.fn(async () => null),
  startMoonTrack: vi.fn(async () => ({ state: 'following', elevation: true, azDeg: null, elDeg: null })),
  pointRotator: vi.fn(() => Promise.resolve()),
  pointRotatorElevation: vi.fn(() => Promise.resolve()),
  pointRotatorAtCall: vi.fn((_call: string, _longPath?: boolean): Promise<PointedAt> => Promise.resolve(POINTED)),
  rotatorBearingToCall: vi.fn((_call: string): Promise<CallBearing> => Promise.resolve(DL1ABC)),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn(() => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
}))
vi.mock('../../api', () => api)
vi.mock('../../toast', () => ({ pushToast: vi.fn() }))

afterEach(cleanup)
beforeEach(() => {
  vi.clearAllMocks()
  api.rotatorBearingToCall.mockImplementation(() => Promise.resolve(DL1ABC))
  api.pointRotatorAtCall.mockImplementation(() => Promise.resolve(POINTED))
})

const point = () => screen.queryByRole('button', { name: t('rotor.pane.aim.point.label') })
const lp = () => screen.queryByRole('button', { name: 'LP' })

describe('the long-path beam button on the Rotor pane', () => {
  it('asks for the reciprocal, and Point still asks for the short path', async () => {
    render(<RotorPane entryCall="DL1ABC" />)
    // Precondition: line 2 has its bearing and its Point, so the pair is there to press.
    await waitFor(() => expect(point()).not.toBeNull())
    expect(lp()).not.toBeNull()

    fireEvent.click(lp()!)
    // After the tracker stop (rotorPointAt stops a pass or a Moon follow first).
    await waitFor(() => expect(api.pointRotatorAtCall).toHaveBeenCalledWith('DL1ABC', true))

    // ⭐ THE CONTROL: adding the reciprocal must not have turned Point into a long-path slew.
    api.pointRotatorAtCall.mockClear()
    fireEvent.click(point()!)
    await waitFor(() => expect(api.pointRotatorAtCall).toHaveBeenCalledWith('DL1ABC', false))
    expect(lp()!.getAttribute('title')).toBe(t('rotor.pane.aim.pointLong.title', { call: 'DL1ABC' }))
  })

  it('offers neither button with no station in play, nor for a station the station cannot place', async () => {
    const { rerender, container } = render(<RotorPane entryCall="DL1ABC" />)
    await waitFor(() => expect(lp()).not.toBeNull())
    rerender(<RotorPane entryCall={null} />)
    expect(lp()).toBeNull()
    expect(point()).toBeNull()
    cleanup()

    // A call the station cannot place: line 2 says so, and offers no slew either way.
    api.rotatorBearingToCall.mockImplementation(() => Promise.reject(new Error('unknownStation')))
    render(<RotorPane entryCall="ZZ9ZZZ" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.aim.unknown'))).not.toBeNull())
    expect(lp()).toBeNull()
    expect(point()).toBeNull()
    expect(api.pointRotatorAtCall).not.toHaveBeenCalled()
    void container
  })

  it('LP while the rotator follows the Moon ends the follow first, then turns the long way', async () => {
    const order: string[] = []
    api.getMoonStatus.mockImplementation(async () => ({
      azDeg: 104.7,
      elDeg: 26.4,
      track: { state: 'following', elevation: true, azDeg: 104.7, elDeg: 26.4 },
      window: null,
    }) as never)
    api.stopSatTrack.mockImplementation(async () => {
      order.push('stop')
    })
    api.pointRotatorAtCall.mockImplementation(async (_c: string, lp?: boolean) => {
      order.push(lp ? 'point long' : 'point short')
      return POINTED
    })
    render(<RotorPane entryCall="DL1ABC" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.moon.following'))).not.toBeNull())
    await waitFor(() => expect(lp()).not.toBeNull())
    fireEvent.click(lp()!)
    await waitFor(() => expect(order).toEqual(['stop', 'point long']))
  })
})
