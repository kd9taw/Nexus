// @vitest-environment jsdom
//
// Two TX-safety affordances the cockpit owes the operator (B7): a toast on the RISING EDGE
// of the idle-watchdog trip (the automatic origins just stood down with no click behind it —
// say so once, not every 500 ms poll), and the pending-reply row's three faces: a countdown
// when the reply CAN key (its origin is armed), "TX is off" when the latch is down, and
// "not armed" when the latch is up but the origin is not (the idle trip).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import type { AppSnapshot, Js8State } from '../types'

const base = (): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
  idleMinutes: 12,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: base() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const s = () => state.current
  return {
    ...auto,
    getJs8State: vi.fn(async () => s()),
    js8Enter: vi.fn(async () => s()),
    js8Send: vi.fn(async () => s()),
    js8SendCommand: vi.fn(async () => s()),
    js8CallCq: vi.fn(async () => s()),
    js8Arm: vi.fn(async () => s()),
    js8Cancel: vi.fn(async () => s()),
    js8DropQueue: vi.fn(async () => s()),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', () => ({
  pushToast: toast.pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  vi.useFakeTimers()
  state.current = base()
  toast.pushToast.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

async function renderCockpit() {
  const r = render(<Js8Cockpit snap={snap} />)
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10)
  })
  return r
}
/** Advance the 500 ms poll and let its promise settle. */
async function poll() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600)
  })
}
const q = <T extends Element>(sel: string) => document.querySelector(sel) as T

describe('the idle-watchdog toast', () => {
  it('fires ONCE on the rising edge of idleTripped and not on later polls', async () => {
    await renderCockpit()
    await poll()
    expect(toast.pushToast).not.toHaveBeenCalled()
    state.current = { ...base(), idleTripped: true }
    await poll()
    expect(toast.pushToast).toHaveBeenCalledTimes(1)
    expect(String(toast.pushToast.mock.calls[0][0])).toMatch(/idle/i)
    expect(String(toast.pushToast.mock.calls[0][0])).toContain('60')
    await poll()
    await poll()
    expect(toast.pushToast).toHaveBeenCalledTimes(1)
    // Clears and trips again → a second toast (a new event, not a repeat).
    state.current = { ...base(), idleTripped: false }
    await poll()
    state.current = { ...base(), idleTripped: true }
    await poll()
    expect(toast.pushToast).toHaveBeenCalledTimes(2)
  })
})

describe('the pending row knows whether its reply can key', () => {
  const pendingReply = { origin: 'autoReply' as const, to: 'W1AW', display: 'KD9TAW: W1AW SNR -05', firesAtMs: Date.now() + 17_000 }

  it('TX off → the "TX is off" face', async () => {
    state.current = { ...base(), pendingReply }
    await renderCockpit()
    expect(q('.js8-pending-row').textContent).toMatch(/TX is off/)
    expect(q('.js8-cancel')).not.toBeNull()
  })

  it('TX on but the origin not armed (idle-tripped) → the "not armed" face', async () => {
    state.current = { ...base(), txEnabled: true, idleTripped: true, pendingReply, armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false } }
    await renderCockpit()
    expect(q('.js8-pending-row').textContent).toMatch(/not armed/i)
    expect(q('.js8-pending-row').textContent).not.toMatch(/TX is off/)
    expect(q('.js8-cancel')).not.toBeNull()
  })

  it('both acts present → the countdown', async () => {
    state.current = { ...base(), txEnabled: true, pendingReply, armed: { autoreply: true, relay: true, hbAck: false, hb: false, cq: false } }
    await renderCockpit()
    expect(q('.js8-pending-row').textContent).toMatch(/\d+ s/)
    expect(q('.js8-pending-row').textContent).not.toMatch(/not armed|TX is off/i)
  })

  it('a relay reply reads the RELAY switch, not the autoreply one', async () => {
    state.current = {
      ...base(),
      txEnabled: true,
      pendingReply: { ...pendingReply, origin: 'relay' },
      armed: { autoreply: true, relay: false, hbAck: false, hb: false, cq: false },
    }
    await renderCockpit()
    expect(q('.js8-pending-row').textContent).toMatch(/not armed/i)
  })
})
