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
import { pointRotatorAtCall } from '../api'
import { pushToast } from '../toast'
import type { PointedAt } from '../types'

vi.mock('../api', () => ({ pointRotatorAtCall: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const mockedPoint = vi.mocked(pointRotatorAtCall)
const mockedToast = vi.mocked(pushToast)

const TO_GRID: PointedAt = { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }
const TO_POSITION: PointedAt = { bearing: 227.1, to: 'position', grid: null, country: null }
const TO_COUNTRY: PointedAt = { bearing: 206.9, to: 'country', grid: null, country: 'Spain' }

beforeEach(() => vi.clearAllMocks())

async function toastFor(result: PointedAt | null, longPath = false): Promise<string> {
  mockedPoint.mockResolvedValue(result as never)
  rotorPointAt(true)('EC1DD', longPath)
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
    rotorPointAt(true)('EC1DD', true)
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
