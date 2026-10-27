// @vitest-environment jsdom
//
// THE STATION ON THE ROSE: line 2's bearing drawn as a second line on the rose, and "N° off" on line 2,
// how far the antenna's live heading is from it. Both are line 2's own answer (`rotatorBearingToCall`,
// the point's resolver read only), so this file holds them to exactly the cases line 2 has a bearing in,
// to the bearing's own angle in the needle's own reference (degrees true), and to leaving the antenna's
// needle exactly as it was. The colour is measured in RotorPane.contrast.test.tsx; the geometry (inside the
// rose at every size, under the needle, no clicks taken) in a real browser.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { RotorPane } from './RotorPane'
import { t } from '../../i18n'
import { css, loadSheets } from '../../cssCascade.testkit'
import type { CallBearing, PointedAt, RotatorState } from '../../types'

vi.setConfig({ testTimeout: 15_000 })

/** EC1DD from JO21EV: its callbook grid IN52TK, 227.4° true and 1530.6 km away. */
const EC1DD: CallBearing = { pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }
/** AA1AA: its log form's grid FN42KH, 290.6° and 5379.2 km away. */
const AA1AA: CallBearing = { pointed: { bearing: 290.6, to: 'grid', grid: 'FN42KH', country: null }, km: 5379.2 }
const at = (bearing: number): CallBearing => ({ ...EC1DD, pointed: { ...EC1DD.pointed, bearing } })
const G5500: RotatorState = { azDeg: 123, reading: 'position', elDeg: 45, elRange: [0, 180] }

// The api the pane may reach, and nothing else.
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

beforeAll(() => loadSheets())
beforeEach(() => {
  vi.clearAllMocks()
  localStorage.setItem('nexus.units', 'metric')
  api.readRotator.mockImplementation(() => Promise.resolve(123))
  api.readRotatorState.mockImplementation(() => Promise.resolve(G5500))
  api.rotatorBearingToCall.mockImplementation((call: string) =>
    answers[call] ? Promise.resolve(answers[call]) : Promise.reject(new Error('unknownStation')),
  )
  api.getDeclination.mockImplementation(() => Promise.resolve(null))
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve(null))
  api.stopSatTrack.mockImplementation(() => Promise.resolve())
  api.getSettings.mockImplementation(() => Promise.resolve({ rotatorModel: 603, rotatorHost: '' } as never))
})
afterEach(() => {
  cleanup()
  localStorage.removeItem('nexus.units')
})

const line = () => document.querySelector<SVGGElement>('.rotor-bearing')
const off = () => document.querySelector<HTMLElement>('.rotor-aim-off')
const needle = () => document.querySelector<SVGLineElement>('.rotor-needle:not(.target)')
const line2To = () => document.querySelector<HTMLElement>('.rotor-aim-to')?.textContent
const attrs = (el: Element) => Object.fromEntries([...el.attributes].map((a) => [a.name, a.value]))
const shown = async () => {
  await waitFor(() => expect(screen.queryByText('123°T')).not.toBeNull())
}
/** An answer the pane asked for has been handled: its handler runs on the next turn. */
const settled = async () => {
  await waitFor(() => expect(api.rotatorBearingToCall).toHaveBeenCalled())
  await new Promise((r) => setTimeout(r, 0))
}

describe('the station on the rose: line 2’s bearing as a second line', () => {
  it('is drawn at the bearing’s own angle, 227.4°, turned about the rose’s centre', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line()).not.toBeNull())
    expect(line()!.getAttribute('transform')).toBe('rotate(227.4 74 74)')
    // Drawn at north and turned: from the hub to the ring (r 64), and its bug on the rim inside the 148 box,
    // so it scales with the rose the way the needles do and never leaves it.
    expect(attrs(line()!.querySelector('.rotor-bearing-line')!)).toMatchObject({ x1: '74', y1: '74', x2: '74', y2: '10' })
    // A triangle with its point on the ring and its corners at (70, 3) and (78, 3): outside the ring's ticks
    // and letters, inside the box.
    expect(line()!.querySelector('.rotor-bearing-bug')?.getAttribute('d'), 'no bug on the rim').toBe('M 74 10 l -4 -7 h 8 z')
  })

  it('is in the needle’s own reference, degrees TRUE: a declination moves the °M reading and not the line', async () => {
    api.getDeclination.mockImplementation(() => Promise.resolve(10))
    render(<RotorPane entryCall="EC1DD" />)
    // The declination reached the pane (the control): the readout gives 113°M beside 123°T…
    await waitFor(() => expect(document.querySelector('.rotor-mag')?.textContent?.trim()).toBe('113°M'))
    await waitFor(() => expect(line()).not.toBeNull())
    // …and the line stays at the true bearing Point sends the rotator, never 217.4 (227.4 − 10).
    expect(line()!.getAttribute('transform')).toBe('rotate(227.4 74 74)')
    // The needle beside it is the rotator's own reading, 123° true, drawn as it stands.
    const rad = ((123 - 90) * Math.PI) / 180
    expect(Number(needle()!.getAttribute('x2'))).toBeCloseTo(74 + 56 * Math.cos(rad), 9)
    expect(Number(needle()!.getAttribute('y2'))).toBeCloseTo(74 + 56 * Math.sin(rad), 9)
  })

  it('follows the entry: a new call moves the line, and never shows the last call’s while it is asked', async () => {
    let answer: (b: CallBearing) => void = () => {}
    const { rerender } = render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line()?.getAttribute('transform')).toBe('rotate(227.4 74 74)'))
    api.rotatorBearingToCall.mockImplementation((call: string) =>
      call === 'AA1AA' ? new Promise<CallBearing>((r) => (answer = r)) : Promise.resolve(answers[call]),
    )
    rerender(<RotorPane entryCall="AA1AA" />)
    await waitFor(() => expect(api.rotatorBearingToCall).toHaveBeenLastCalledWith('AA1AA'))
    expect(line2To()).toBe('→ AA1AA')
    expect(line(), 'EC1DD’s line stayed up for AA1AA').toBeNull()
    expect(off()).toBeNull()
    answer(AA1AA)
    await waitFor(() => expect(line()?.getAttribute('transform')).toBe('rotate(290.6 74 74)'))
  })

  it('leaves the antenna’s needle exactly as it was, and under it: the needle stays on top', async () => {
    render(<RotorPane />)
    await shown()
    const plain = attrs(needle()!)
    expect(plain.transform, 'the needle is drawn by its end point, never turned').toBeUndefined()
    cleanup()
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line()).not.toBeNull())
    expect(attrs(needle()!)).toEqual(plain)
    // Painted in document order: the line first, so both needles are drawn over it.
    for (const n of document.querySelectorAll('.rotor-needle'))
      expect(line()!.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING, 'a needle is drawn under the line').toBeTruthy()
  })

  it('takes no clicks: a click on it reaches the rose underneath, which slews', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(line()).not.toBeNull())
    // A browser never makes it a click's target…
    expect(css(line()!, 'pointer-events')).toBe('none')
    // …and nothing on it stops a click on its way to the rose (jsdom ignores pointer-events).
    fireEvent.click(line()!.querySelector('.rotor-bearing-line')!)
    await waitFor(() => expect(api.pointRotator).toHaveBeenCalledTimes(1))
  })
})

describe('no bearing, no line and no number — as line 2 has it', () => {
  it('no call: neither, and the station is asked nothing', async () => {
    render(<RotorPane />)
    await shown()
    expect(line()).toBeNull()
    expect(off()).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })

  it('a station nothing places (unknownStation)', async () => {
    render(<RotorPane entryCall="QQ1QQ" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.aim.unknown'))).not.toBeNull())
    expect(line()).toBeNull()
    expect(off()).toBeNull()
  })

  it('no grid of your own (noGrid)', async () => {
    api.rotatorBearingToCall.mockImplementation(() => Promise.reject('noGrid'))
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(screen.queryByText(t('rotor.pane.aim.noGrid'))).not.toBeNull())
    expect(line()).toBeNull()
    expect(off()).toBeNull()
  })

  it('a read that failed without a reason', async () => {
    api.rotatorBearingToCall.mockImplementation(() => Promise.reject(new Error('engine busy')))
    render(<RotorPane entryCall="EC1DD" />)
    await shown()
    await settled()
    expect(line2To()).toBe('→ EC1DD')
    expect(line()).toBeNull()
    expect(off()).toBeNull()
  })
})

describe('"N° off" on line 2: the short way round from the live heading to the bearing', () => {
  for (const [heading, bearing, deg] of [
    [350, 10, 20],
    [10, 350, 20],
    [90, 270, 180],
    [45, 45, 0],
  ] as const) {
    it(`heading ${heading}°, bearing ${bearing}°: ${deg}° off`, async () => {
      api.readRotatorState.mockImplementation(() => Promise.resolve({ ...G5500, azDeg: heading }))
      api.rotatorBearingToCall.mockImplementation(() => Promise.resolve(at(bearing)))
      render(<RotorPane entryCall="EC1DD" />)
      await waitFor(() => expect(off()?.textContent).toBe(t('rotor.pane.aim.off', { deg })))
      expect(off()!.getAttribute('title')).toBe(t('rotor.pane.aim.off.title', { call: 'EC1DD', deg }))
    })
  }

  it('from the two whole numbers the pane shows, so the three agree: 350°T and 10° read 20° off, not 21', async () => {
    api.readRotatorState.mockImplementation(() => Promise.resolve({ ...G5500, azDeg: 349.6 }))
    api.rotatorBearingToCall.mockImplementation(() => Promise.resolve(at(10.4)))
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(off()?.textContent).toBe(t('rotor.pane.aim.off', { deg: 20 })))
    expect(screen.queryByText('350°T')).not.toBeNull()
    expect(line2To()).toBe('→ EC1DD 10° (1531 km)')
  })

  it('moves with the antenna on the next reading', async () => {
    render(<RotorPane entryCall="EC1DD" />)
    await waitFor(() => expect(off()?.textContent).toBe(t('rotor.pane.aim.off', { deg: 104 })))
    api.readRotatorState.mockImplementation(() => Promise.resolve({ ...G5500, azDeg: 220 }))
    await waitFor(() => expect(off()?.textContent).toBe(t('rotor.pane.aim.off', { deg: 7 })), { timeout: 6_000 })
  })

  for (const reading of ['noPosition', 'notAnswering'] as const) {
    it(`no number with no live heading (${reading}), while the line still shows the bearing`, async () => {
      api.readRotatorState.mockImplementation(() => Promise.resolve({ azDeg: null, reading, elDeg: null, elRange: null }))
      render(<RotorPane entryCall="EC1DD" />)
      // The control: the bearing arrived and is drawn.
      await waitFor(() => expect(line()?.getAttribute('transform')).toBe('rotate(227.4 74 74)'))
      expect(screen.queryByText('—°T')).not.toBeNull()
      expect(off()).toBeNull()
      // Nor anywhere else on line 2: the call, the bearing and distance, and Point.
      expect(document.querySelector('.rotor-aim')!.textContent).toBe('→ EC1DD 227° (1531 km)' + t('rotor.pane.aim.point.label'))
    })
  }
})
