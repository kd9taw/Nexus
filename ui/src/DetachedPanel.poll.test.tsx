// @vitest-environment jsdom
//
// #335, the pop-out's half: the Needed pop-out opens by default, and besides the snapshot it polls
// propagation (10 s), the needs board and Settings (15 s each) — all three take the engine mutex,
// and a propagation read can run for seconds behind a slow fetch. On bare intervals, a stall
// stacked a read per tick per poll, each holding a backend worker. They are single-flight now.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import * as api from './api'

const held = vi.hoisted(() => ({ prop: [] as ((v: unknown) => void)[] }))

vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  }
  out.subscribeSnapshot = vi.fn(() => () => {})
  out.getBandPlan = vi.fn().mockResolvedValue([])
  // The three polls under test: none of them answers until the test says so.
  out.getPropagation = vi.fn(() => new Promise((resolve) => held.prop.push(resolve)))
  out.getNeedAlerts = vi.fn(() => new Promise(() => {}))
  out.getSettings = vi.fn(() => new Promise(() => {}))
  return out
})
vi.mock('./toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
  subscribeToasts: vi.fn(() => () => {}),
  dismissToast: vi.fn(),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
beforeEach(() => {
  vi.useFakeTimers()
  held.prop = []
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('the Needed pop-out polls one call at a time (#335)', () => {
  it('asks each stalled read once, and resumes a read when it answers', async () => {
    render(<DetachedPanel panel="needed" />)
    // 40 s: inside every poll's watchdog (four intervals — 40 s for propagation, 60 s for the
    // others), so no call is given up yet and each read is asked for exactly once.
    await act(() => vi.advanceTimersByTimeAsync(40_000))
    expect(api.getPropagation, 'propagation reads stacked every 10 s').toHaveBeenCalledTimes(1)
    expect(api.getNeedAlerts, 'needs reads stacked every 15 s').toHaveBeenCalledTimes(1)
    expect(api.getSettings, 'Settings reads stacked every 15 s').toHaveBeenCalledTimes(1)

    await act(async () => {
      held.prop[0](null)
      await vi.advanceTimersByTimeAsync(10_000)
    })
    expect(api.getPropagation).toHaveBeenCalledTimes(2)
  })
})
