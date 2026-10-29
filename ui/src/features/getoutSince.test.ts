// @vitest-environment jsdom
//
// The Getting Out box's Start fresh time is kept per window and across a restart (features/getoutSince).
// A "restart" is a fresh module: the value must come back from storage, not from memory.
import { describe, it, expect, beforeEach, vi } from 'vitest'

const KEY = 'nexus.connect.getout.since'

async function fresh(search = '') {
  window.history.replaceState(null, '', `/${search}`)
  vi.resetModules()
  return import('./getoutSince')
}

beforeEach(() => {
  localStorage.clear()
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
    m.setGetoutSince(1_800_000_000)
    m.setGetoutSince(null)
    expect(m.getoutSince()).toBeNull()
    expect((await fresh()).getoutSince()).toBeNull()
  })

  it('a Connect pop-out starts on the main window\'s time, then keeps its own', async () => {
    localStorage.setItem(KEY, '1800000000')
    const pop = await fresh('?panel=connect')
    expect(pop.getoutSince(), 'inherited on first open').toBe(1_800_000_000)
    pop.setGetoutSince(null)
    expect(localStorage.getItem(KEY), 'the main window keeps its test').toBe('1800000000')
    expect((await fresh('?panel=connect')).getoutSince(), 'Show all in the pop-out sticks').toBeNull()
    expect((await fresh()).getoutSince(), 'and the main window still counts from its start').toBe(1_800_000_000)
  })

  it('ignores a stored value that is not a time', async () => {
    localStorage.setItem(KEY, 'garbage')
    expect((await fresh()).getoutSince()).toBeNull()
  })
})
