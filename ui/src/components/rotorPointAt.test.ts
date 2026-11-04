// @vitest-environment jsdom
//
// The point-at-call toast says what the bearing was taken to (a tester's report, 2026-09-29).
//
// Every "→ CALL" used to aim at the centre of the station's country and say only the bearing:
// 207° for EC1DD from JO21EV, 20° off the station's own grid, with nothing on screen to show it.
// It now aims at the station when Nexus knows where it is, and the toast names which it got — the
// station's grid, its callbook position, or only the country centre — on both paths. A browser
// gets nothing back (the station resolves the bearing) and its toast is unchanged.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { rotorPointAt, pointedTo } from './rotorPointAt'
import { getMoonStatus, getSatTrackStatus, pointRotatorAtCall, stopSatTrack } from '../api'
import { pushToast } from '../toast'
import type { PointedAt } from '../types'

vi.mock('../api', () => ({
  pointRotatorAtCall: vi.fn(),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTrackStatus: vi.fn(async () => null),
  getMoonStatus: vi.fn(async () => null),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockedPoint = vi.mocked(pointRotatorAtCall)
const mockedToast = vi.mocked(pushToast)

const TO_GRID: PointedAt = { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }
const TO_POSITION: PointedAt = { bearing: 227.1, to: 'position', grid: null, country: null }
const TO_COUNTRY: PointedAt = { bearing: 206.9, to: 'country', grid: null, country: 'Spain' }

beforeEach(() => vi.clearAllMocks())

async function toastFor(result: PointedAt | null, longPath = false): Promise<string> {
  mockedPoint.mockResolvedValue(result as never)
  rotorPointAt()('EC1DD', longPath)
  await vi.waitFor(() => expect(mockedToast).toHaveBeenCalled())
  return String(mockedToast.mock.calls[0][0])
}

describe('the point-at-call toast names what it aimed at', () => {
  it('the station’s own grid', async () => {
    expect(await toastFor(TO_GRID)).toBe('Rotator → EC1DD: 227° (their grid IN52TK)')
  })

  it('the position its callbook gives', async () => {
    expect(await toastFor(TO_POSITION)).toBe('Rotator → EC1DD: 227° (their callbook position)')
  })

  it('only the centre of its country, and says no grid is known', async () => {
    expect(await toastFor(TO_COUNTRY)).toBe(
      'Rotator → EC1DD: 207° (the centre of Spain: no grid known for them)',
    )
  })

  it('on the long path too', async () => {
    mockedPoint.mockResolvedValue({ ...TO_GRID, bearing: 47.4 } as never)
    rotorPointAt()('EC1DD', true)
    await vi.waitFor(() => expect(mockedToast).toHaveBeenCalled())
    expect(mockedPoint).toHaveBeenCalledWith('EC1DD', true)
    expect(String(mockedToast.mock.calls[0][0])).toBe(
      '↗ Pointing antenna long path to 47° (EC1DD, their grid IN52TK)',
    )
  })

  it('a browser, which gets nothing back, keeps its own words', async () => {
    const text = await toastFor(null)
    expect(text).not.toMatch(/grid|centre|callbook/)
    expect(text).toContain('EC1DD')
  })

  it('pointedTo is the one sentence all three toasts share', () => {
    expect([pointedTo(TO_GRID), pointedTo(TO_POSITION), pointedTo(TO_COUNTRY)]).toEqual([
      'their grid IN52TK',
      'their callbook position',
      'the centre of Spain: no grid known for them',
    ])
  })
})

describe('a failed point-at-call keeps each path’s own words', () => {
  it('names the rotator and its error', async () => {
    mockedPoint.mockRejectedValue(new Error('no rotator'))
    rotorPointAt()('EC1DD')
    await vi.waitFor(() => expect(mockedToast).toHaveBeenCalledTimes(1))
    expect(mockedToast.mock.calls.map((call) => call[0])).toEqual(['Rotator: no rotator'])
  })
})

describe('a point ends the tracker steering the mast, and only that', () => {
  const run = async (sat: unknown, moon: unknown, longPath = false) => {
    const order: string[] = []
    vi.mocked(getSatTrackStatus).mockResolvedValue(sat as never)
    vi.mocked(getMoonStatus).mockResolvedValue(moon as never)
    vi.mocked(stopSatTrack).mockImplementation(async () => {
      order.push('stop')
    })
    mockedPoint.mockImplementation(async (_c: string, lp?: boolean) => {
      order.push(lp ? 'point long' : 'point short')
      return TO_GRID
    })
    rotorPointAt()('EC1DD', longPath)
    await vi.waitFor(() => expect(order.some((o) => o.startsWith('point'))).toBe(true))
    return order
  }
  const pass = (state: string) => ({ name: 'SO-50', state })
  const following = { azDeg: 100, elDeg: 20, track: { state: 'following' }, window: null }

  it('a pass that is steering is stopped first, then the point goes, on both paths', async () => {
    expect(await run(pass('tracking'), null, true)).toEqual(['stop', 'point long'])
    expect(await run(pass('prepositioning'), null)).toEqual(['stop', 'point short'])
  })

  it('a pass that lost its rotor while it was steering is stopped, though its badge reads armed', async () => {
    expect(await run({ name: 'SO-50', state: 'armed', rotorLost: true }, null)).toEqual(['stop', 'point short'])
  })

  it('a state that cannot be read is taken as steering: the stop is made', async () => {
    const order: string[] = []
    vi.mocked(getSatTrackStatus).mockRejectedValue(new Error('offline'))
    vi.mocked(getMoonStatus).mockResolvedValue(null as never)
    vi.mocked(stopSatTrack).mockImplementation(async () => {
      order.push('stop')
    })
    mockedPoint.mockImplementation(async () => {
      order.push('point')
      return TO_GRID
    })
    rotorPointAt()('EC1DD')
    await vi.waitFor(() => expect(order).toEqual(['stop', 'point']))
  })

  it('a Moon follow is stopped first', async () => {
    expect(await run(null, following, true)).toEqual(['stop', 'point long'])
  })

  it('an ARMED pass, waiting for AOS with the rotor left to you for HF, stays armed', async () => {
    expect(await run(pass('armed'), null)).toEqual(['point short'])
    // …and with nothing tracking at all, nothing is stopped either.
    expect(await run(null, { azDeg: 100, elDeg: 20, track: null, window: null })).toEqual(['point short'])
  })
})
