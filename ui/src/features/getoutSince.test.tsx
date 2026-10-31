// @vitest-environment jsdom
//
// The Getting Out box's Start fresh time is kept per window and across a restart (features/getoutSince).
// A "restart" is a fresh module: the value must come back from storage, not from memory.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'

const KEY = 'nexus.connect.getout.since'
// The clock every test runs at, so a start time stays inside (or outside) the 30-minute window
// whatever day the suite runs on.
const NOW_MS = Date.UTC(2026, 9, 30, 14, 5, 30)
const NOW = NOW_MS / 1000

async function fresh(search = '') {
  window.history.replaceState(null, '', `/${search}`)
  vi.resetModules()
  return import('./getoutSince')
}

beforeEach(() => {
  localStorage.clear()
  vi.useFakeTimers({ now: NOW_MS })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('the Start fresh time', () => {
  it('is none until pressed', async () => {
    const m = await fresh()
    expect(m.getoutSince()).toBeNull()
  })

  it('Start fresh takes the time now, and a restart keeps it', async () => {
    const m = await fresh()
    m.startGetoutFresh(Date.UTC(2026, 9, 30, 14, 5, 30))
    const at = Date.UTC(2026, 9, 30, 14, 5, 30) / 1000
    expect(m.getoutSince()).toBe(at)
    expect(localStorage.getItem(KEY)).toBe(String(at))
    expect((await fresh()).getoutSince(), 'after a restart').toBe(at)
  })

  it('Show all clears it, and a restart keeps it cleared', async () => {
    const m = await fresh()
    m.setGetoutSince(NOW)
    m.setGetoutSince(null)
    expect(m.getoutSince()).toBeNull()
    expect((await fresh()).getoutSince()).toBeNull()
  })

  it('a Connect pop-out starts on the main window\'s time, then keeps its own', async () => {
    localStorage.setItem(KEY, String(NOW))
    const pop = await fresh('?panel=connect')
    expect(pop.getoutSince(), 'inherited on first open').toBe(NOW)
    pop.setGetoutSince(null)
    expect(localStorage.getItem(KEY), 'the main window keeps its test').toBe(String(NOW))
    expect((await fresh('?panel=connect')).getoutSince(), 'Show all in the pop-out sticks').toBeNull()
    expect((await fresh()).getoutSince(), 'and the main window still counts from its start').toBe(NOW)
  })

  it('ignores a stored value that is not a time', async () => {
    localStorage.setItem(KEY, 'garbage')
    expect((await fresh()).getoutSince()).toBeNull()
  })

  it('keeps a press when the store is blocked, for the rest of the session', async () => {
    const m = await fresh()
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    m.startGetoutFresh(NOW_MS)
    expect(m.getoutSince()).toBe(NOW)
    expect(localStorage.getItem(KEY), 'nothing was stored').toBeNull()
    m.setGetoutSince(null)
    expect(m.getoutSince(), 'and Show all clears it the same way').toBeNull()
  })

  it('ignores a stored value that is not a whole second', async () => {
    localStorage.setItem(KEY, String(NOW - 0.5))
    expect((await fresh()).getoutSince()).toBeNull()
    localStorage.setItem(KEY, String(NOW))
    expect((await fresh()).getoutSince(), 'control: a whole second inside the window').toBe(NOW)
  })
})

// The box shows the station's last 30 minutes; a start older than that would make "since HH:MM:SS"
// untrue, so Start fresh ends by itself and the box is back to its last 30 minutes.
describe('Start fresh ends by itself after the 30-minute window', () => {
  it('holds through the 30th minute and ends after it', async () => {
    const m = await fresh()
    m.startGetoutFresh(NOW_MS)
    expect(m.getoutSince((NOW + 1800) * 1000), 'exactly 30 minutes on').toBe(NOW)
    expect(m.getoutSince((NOW + 1801) * 1000), 'past the window').toBeNull()
  })

  it('a restart inside the window keeps it; a restart after the window does not', async () => {
    const m = await fresh()
    m.startGetoutFresh(NOW_MS)
    vi.setSystemTime((NOW + 1799) * 1000)
    expect((await fresh()).getoutSince(), 'restarted at 29:59').toBe(NOW)
    vi.setSystemTime((NOW + 1801) * 1000)
    expect((await fresh()).getoutSince(), 'restarted at 30:01').toBeNull()
  })

  it('an open box goes back to every report by itself when the window passes', async () => {
    const m = await fresh()
    const seen: (number | null)[] = []
    function Probe() {
      seen.push(m.useGetoutSince())
      return null
    }
    render(<Probe />)
    act(() => m.startGetoutFresh(NOW_MS))
    expect(seen[seen.length - 1]).toBe(NOW)
    act(() => {
      vi.advanceTimersByTime(1800 * 1000)
    })
    expect(seen[seen.length - 1], 'still inside at 30:00').toBe(NOW)
    act(() => {
      vi.advanceTimersByTime(1000)
    })
    expect(seen[seen.length - 1], 'ended without a press').toBeNull()
  })
})
