// @vitest-environment jsdom
//
// THE AUTO-REPEATING CQ AND HEARTBEAT — JS8Call's checkable `cqMacroButton` / `hbMacroButton`
// with the countdown rendered IN the button (`updateRepeatButtonDisplay`, mainwindow.cpp:7800:
// `CQ (12)`, `HB (42)`, `HB (now)`). Two properties are load-bearing here and neither is
// cosmetic:
//
//   1. THE COUNTDOWN IS ON THE CONTROL. A beacon operator walks away from this screen; the
//      only thing that says when the next call goes out is the button itself.
//   2. A REPEAT THAT IS ON BUT CANNOT KEY MUST NOT LOOK ARMED. The three faces of a
//      second-act chip (off · on-but-TX-off · ARMED) apply to the repeat exactly as they do
//      to AUTOREPLY/RELAY/HB ACK, because the repeat IS a second act — the session TX latch
//      is the first, and the engine re-reads it every slot.
//
// Nothing in this file can key: every handler is an engine call through the mocked api.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
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
  idleMinutes: 3,
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
    js8CallCq: vi.fn(async () => s()),
    js8CqRepeat: vi.fn(async () => s()),
    js8Arm: vi.fn(async () => s()),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const js8CallCq = api.js8CallCq as ReturnType<typeof vi.fn>
const js8CqRepeat = api.js8CqRepeat as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  js8CallCq.mockClear()
  js8CqRepeat.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit() {
  const r = render(<Js8Cockpit snap={snap} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}
const q = <T extends Element>(sel: string) => document.querySelector(sel) as T
const label = (sel: string) => q(sel).querySelector('.cw-macro-label')?.textContent ?? ''

describe('the CQ button is a one-shot at interval 0 and the repeat above it', () => {
  it('sends ONE CQ and is not a toggle while the interval is 0', async () => {
    await renderCockpit()
    const btn = q<HTMLButtonElement>('.js8-cq')
    expect(btn.getAttribute('aria-pressed')).toBeNull()
    expect(label('.js8-cq')).toBe('CQ')
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(js8CallCq).toHaveBeenCalledWith(0)
    expect(js8CqRepeat).not.toHaveBeenCalled()
  })

  it('becomes the checkable repeat once an interval is set, and arms with the chosen CQ variant', async () => {
    state.current = { ...base(), cqIntervalMin: 10 }
    await renderCockpit()
    fireEvent.change(q<HTMLSelectElement>('.js8-cq-select'), { target: { value: '3' } })
    const btn = q<HTMLButtonElement>('.js8-cq')
    expect(btn.getAttribute('aria-pressed')).toBe('false')
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(js8CqRepeat).toHaveBeenCalledWith(true, 3)
    expect(js8CallCq).not.toHaveBeenCalled()
  })

  it('turning an armed repeat off asks the engine to disarm', async () => {
    state.current = { ...base(), cqIntervalMin: 10, cqOn: true, cqNextAtMs: Date.now() + 60_000 }
    await renderCockpit()
    await act(async () => {
      fireEvent.click(q('.js8-cq'))
    })
    expect(js8CqRepeat).toHaveBeenCalledWith(false, 0)
  })
})

describe('the countdown is rendered IN the button, as JS8Call does it', () => {
  it('counts the seconds to the next CQ', async () => {
    state.current = { ...base(), cqIntervalMin: 5, cqOn: true, cqNextAtMs: Date.now() + 12_400 }
    await renderCockpit()
    expect(label('.js8-cq')).toBe('CQ (12)')
  })

  it('counts the seconds to the next heartbeat', async () => {
    state.current = { ...base(), hbIntervalMin: 10, hbOn: true, hbNextAtMs: Date.now() + 42_900 }
    await renderCockpit()
    expect(label('.js8-hb')).toBe('HB (42)')
  })

  it('reads "now" once the deadline has passed rather than a negative number', async () => {
    state.current = { ...base(), hbIntervalMin: 10, hbOn: true, hbNextAtMs: Date.now() - 3_000 }
    await renderCockpit()
    expect(label('.js8-hb')).toBe('HB (now)')
  })

  it('shows the bare token when nothing is scheduled — no empty parentheses', async () => {
    state.current = { ...base(), cqIntervalMin: 5, hbIntervalMin: 5 }
    await renderCockpit()
    expect(label('.js8-cq')).toBe('CQ')
    expect(label('.js8-hb')).toBe('HB')
  })

  it('does not count down a schedule the engine reports as off', async () => {
    // A stale `cqNextAtMs` with the switch off must not paint a running countdown: the
    // button is the only place the operator reads whether the station is calling.
    state.current = { ...base(), cqIntervalMin: 5, cqOn: false, cqNextAtMs: Date.now() + 30_000 }
    await renderCockpit()
    expect(label('.js8-cq')).toBe('CQ')
  })
})

describe('the three faces: a repeat that cannot key must not look armed', () => {
  it('on-but-TX-off is `on` and NOT `armed`', async () => {
    state.current = { ...base(), cqIntervalMin: 5, cqOn: true, cqNextAtMs: Date.now() + 30_000, txEnabled: false }
    await renderCockpit()
    const btn = q<HTMLButtonElement>('.js8-cq')
    expect(btn.className).toContain('on')
    expect(btn.className).not.toContain('armed')
    expect(btn.title).toMatch(/TX is off/i)
  })

  it('both acts present is `armed`', async () => {
    state.current = {
      ...base(),
      cqIntervalMin: 5,
      cqOn: true,
      cqNextAtMs: Date.now() + 30_000,
      txEnabled: true,
      armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: true },
    }
    await renderCockpit()
    expect(q('.js8-cq').className).toContain('armed')
  })

  it('the idle watchdog having tripped shows on-but-not-armed, never armed', async () => {
    state.current = {
      ...base(),
      cqIntervalMin: 5,
      cqOn: true,
      cqNextAtMs: Date.now() + 30_000,
      txEnabled: true,
      idleTripped: true,
      armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
    }
    await renderCockpit()
    const btn = q<HTMLButtonElement>('.js8-cq')
    expect(btn.className).toContain('on')
    expect(btn.className).not.toContain('armed')
  })
})

describe('the repeat controls live outside every removable pane', () => {
  it('the CQ and HB buttons are in the TX dock, which carries no ⊞ id', async () => {
    state.current = { ...base(), cqIntervalMin: 5 }
    await renderCockpit()
    expect(q('.js8-cq').closest('.cockpit-txdock')).not.toBeNull()
    expect(q('.js8-hb').closest('.cockpit-txdock')).not.toBeNull()
    expect(q('.js8-cq').closest('.cockpit-pane')).toBeNull()
    expect(q('.js8-hb').closest('.cockpit-pane')).toBeNull()
  })
})
