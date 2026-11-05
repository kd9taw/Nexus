// @vitest-environment jsdom
//
// The JS8 dock's heartbeat controls, as JS8Call 3.0.3 has them (components/Js8Heartbeat):
//   · HB NET (heartbeat networking): HB exists only while it is on, and not at JS8 40, where HB NET
//     is disabled and says why. A sample with no `presence` (the Remote page's) shows HB as before.
//   · HB's menu (▾, or a right-click on HB): "Send heartbeat now", and with an interval set,
//     "Start / Stop heartbeat timer"; the dock is lifted while the menu stands on it.
//   · The QSO pause: the To box is the selected call, told to the station as it changes; turning
//     HB on with the pause on clears it, as JS8Call's HB button clears its selected call.
//   · HB ACK armed but held back right now reads as not armed and says why; the 55-minute rule's
//     stations are named on it. @ALLCALL leaves the To list when not taking part in it.
// What keys is the engine's (crates/js8 station tests, tempo-app js8 tests); this pins what the
// dock shows and which verb each control calls.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { keepInSight } from './CockpitTxStrip'
import { Js8Cockpit } from './Js8Cockpit'
import { t } from '../i18n'
import type { AppSnapshot, Js8Presence, Js8State } from '../types'

const presence = (over: Partial<Js8Presence> = {}): Js8Presence => ({
  hbNetworking: true,
  hbSpeed: true,
  hbPaused: false,
  hbAckSuppressed: null,
  hbAckRateBlocked: [],
  qsoPause: true,
  avoidAllcall: false,
  ...over,
})
const base = (over: Partial<Js8State> = {}): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: true,
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
  armed: { autoreply: true, relay: true, hbAck: false, hb: false, cq: false },
  notArmed: { autoreply: null, relay: null, hbAck: null, hb: null, cq: null },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
  presence: presence(),
  ...over,
})
const state: { current: Js8State } = { current: base() }

const spies = vi.hoisted(() => ({
  js8Arm: vi.fn(),
  js8HeartbeatNow: vi.fn(),
  js8SelectCall: vi.fn(),
}))
vi.mock('./CockpitTxStrip', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./CockpitTxStrip')>()),
  keepInSight: vi.fn(),
}))
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
    js8Arm: spies.js8Arm,
    js8HeartbeatNow: spies.js8HeartbeatNow,
    js8SelectCall: spies.js8SelectCall,
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

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  vi.useFakeTimers()
  state.current = base()
  for (const spy of Object.values(spies)) {
    spy.mockReset()
    spy.mockImplementation(async () => state.current)
  }
  spies.js8SelectCall.mockImplementation(async () => undefined)
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

async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600)
  })
}
async function renderCockpit(s: Js8State) {
  state.current = s
  render(<Js8Cockpit snap={snap} />)
  await settle()
}
const q = (sel: string) => document.querySelector(sel) as HTMLElement | null
const differs = ` ${t('js8.dock.arm.differs')}`
const toBox = () => q('.js8-to') as HTMLInputElement
const menuItems = () => screen.queryAllByRole('menuitem').map((b) => b.textContent)

describe('HB NET: heartbeat networking, and HB only while it is on', () => {
  it('networking off: HB NET reads off, there is no HB, and a click turns networking on', async () => {
    await renderCockpit(base({ presence: presence({ hbNetworking: false }) }))
    const net = q('.js8-hbnet')!
    expect(net.getAttribute('aria-pressed')).toBe('false')
    expect(net.title).toBe(t('js8.dock.hbNet.title.off'))
    expect(q('.js8-hb'), 'no HB button without heartbeat networking').toBeNull()
    fireEvent.click(net)
    await settle()
    expect(spies.js8Arm).toHaveBeenCalledWith('hbnet', true)
  })

  it('networking on: HB NET reads on and HB is there', async () => {
    await renderCockpit(base())
    expect(q('.js8-hbnet')!.getAttribute('aria-pressed')).toBe('true')
    expect(q('.js8-hbnet')!.title).toBe(t('js8.dock.hbNet.title.on'))
    expect(q('.js8-hb')).not.toBeNull()
    fireEvent.click(q('.js8-hbnet')!)
    await settle()
    expect(spies.js8Arm).toHaveBeenCalledWith('hbnet', false)
  })

  it('at JS8 40 (once Turbo): no HB, and HB NET is disabled and says why, keeping its setting', async () => {
    await renderCockpit(base({ speed: 'turbo', presence: presence({ hbSpeed: false }) }))
    expect(q('.js8-hb')).toBeNull()
    const net = q('.js8-hbnet') as HTMLButtonElement
    expect(net.disabled).toBe(true)
    expect(net.getAttribute('aria-pressed')).toBe('true')
    expect(net.title).toBe(t('js8.dock.hbNet.title.speed', { speed: 'JS8 40' }))
  })

  it('a sample with no presence (the Remote page) shows HB as it always was, and no HB NET', async () => {
    await renderCockpit(base({ presence: undefined }))
    expect(q('.js8-hb')).not.toBeNull()
    expect(q('.js8-hbnet')).toBeNull()
  })
})

describe('CQ and HB wait while a message is queued or going out', () => {
  // JS8Call's `updateButtonDisplay` greys both while `isMessageQueuedForTransmit()`
  // (mainwindow.cpp:5675-5689, :3264-3266): something waiting to go, or the key down.
  const greyed = () => ['.js8-cq', '.js8-hb', '.js8-hb-menu-btn'].map((s) => (q(s) as HTMLButtonElement).disabled)
  const waiting = [{ origin: 'autoReply' as const, display: 'KD9TAW: W1AW SNR -05', first: true, last: true }]

  beforeEach(async () => {
    const api = await import('../api')
    vi.mocked(api.js8CallCq).mockReset()
    vi.mocked(api.js8CallCq).mockImplementation(async () => state.current)
  })

  it('a message waiting in the queue greys CQ, HB and HB’s menu, and a click on them sends nothing', async () => {
    const api = await import('../api')
    await renderCockpit(base({ queue: waiting }))
    expect(greyed()).toEqual([true, true, true])
    fireEvent.click(q('.js8-cq')!)
    fireEvent.click(q('.js8-hb')!)
    await settle()
    expect(api.js8CallCq).not.toHaveBeenCalled()
    expect(spies.js8Arm).not.toHaveBeenCalled()
  })

  it('the key down with the queue empty greys them too, and the repeating CQ with them', async () => {
    await renderCockpit(base({ sending: true, cqIntervalMin: 5 }))
    expect(q('.js8-cq-repeat'), 'premise: CQ repeats').not.toBeNull()
    expect(greyed()).toEqual([true, true, true])
  })

  it('control: with nothing queued or going out, both can be pressed, and the same clicks send', async () => {
    const api = await import('../api')
    await renderCockpit(base())
    expect(greyed()).toEqual([false, false, false])
    fireEvent.click(q('.js8-cq')!)
    fireEvent.click(q('.js8-hb')!)
    await settle()
    expect(api.js8CallCq).toHaveBeenCalledTimes(1)
    expect(spies.js8Arm).toHaveBeenCalledWith('hb', true)
  })
})

describe('HB’s menu: send one now, start or stop the timer', () => {
  it('▾ opens it; at interval 0 it offers Send heartbeat now alone, which sends one', async () => {
    await renderCockpit(base())
    expect(screen.queryByRole('menu')).toBeNull()
    fireEvent.click(q('.js8-hb-menu-btn')!)
    expect(menuItems()).toEqual([t('js8.dock.hb.menu.now')])
    fireEvent.click(screen.getByRole('menuitem', { name: t('js8.dock.hb.menu.now') }))
    await settle()
    expect(spies.js8HeartbeatNow).toHaveBeenCalledTimes(1)
    expect(spies.js8Arm, 'sending one now leaves the repeat alone').not.toHaveBeenCalled()
    expect(screen.queryByRole('menu'), 'the pick closes the menu').toBeNull()
  })

  it('a right-click on HB opens it; with an interval it starts the timer, and stops it once on', async () => {
    await renderCockpit(base({ hbIntervalMin: 5 }))
    fireEvent.contextMenu(q('.js8-hb')!)
    expect(menuItems()).toEqual([t('js8.dock.hb.menu.start'), t('js8.dock.hb.menu.now')])
    fireEvent.click(screen.getByRole('menuitem', { name: t('js8.dock.hb.menu.start') }))
    await settle()
    expect(spies.js8Arm).toHaveBeenLastCalledWith('hb', true)
    state.current = base({ hbIntervalMin: 5, hbOn: true, hbNextAtMs: Date.now() + 300_000 })
    await settle()
    fireEvent.contextMenu(q('.js8-hb')!)
    expect(menuItems()).toEqual([t('js8.dock.hb.menu.stop'), t('js8.dock.hb.menu.now')])
    fireEvent.click(screen.getByRole('menuitem', { name: t('js8.dock.hb.menu.stop') }))
    await settle()
    expect(spies.js8Arm).toHaveBeenLastCalledWith('hb', false)
  })

  it('its first item is brought into view as it opens: folded, the menu is in the flow, and opened under the keep row', async () => {
    const seen: Array<[string, unknown]> = []
    const was = Element.prototype.scrollIntoView
    Element.prototype.scrollIntoView = function (this: Element, arg?: boolean | ScrollIntoViewOptions) {
      seen.push([this.getAttribute('role') ?? '', arg])
    }
    try {
      await renderCockpit(base())
      fireEvent.click(q('.js8-hb-menu-btn')!)
      expect(seen, 'the menu opened wherever it landed').toEqual([['menuitem', { block: 'nearest' }]])
    } finally {
      Element.prototype.scrollIntoView = was
    }
  })

  it('…and it is kept in sight where the room is shorter than the item', async () => {
    const was = Element.prototype.scrollIntoView
    Element.prototype.scrollIntoView = () => {}
    try {
      vi.mocked(keepInSight).mockClear()
      await renderCockpit(base())
      fireEvent.click(q('.js8-hb-menu-btn')!)
      expect(vi.mocked(keepInSight).mock.calls.map(([e]) => (e as HTMLElement).getAttribute('role')), 'the menu’s first item can stay behind the keep row').toEqual(['menuitem'])
    } finally {
      Element.prototype.scrollIntoView = was
    }
  })

  it('the dock is lifted while the menu stands on it, and set down when it closes', async () => {
    await renderCockpit(base())
    const dock = q('.cockpit-txdock')!
    expect(dock.classList.contains('cockpit-txdock-editing')).toBe(false)
    fireEvent.click(q('.js8-hb-menu-btn')!)
    expect(dock.classList.contains('cockpit-txdock-editing'), 'open: lifted').toBe(true)
    fireEvent.pointerDown(document.body)
    expect(screen.queryByRole('menu'), 'a click elsewhere closes it').toBeNull()
    expect(dock.classList.contains('cockpit-txdock-editing'), 'closed: back at its level').toBe(false)
  })
})

describe('the QSO pause: the To box is the selected call', () => {
  it('the To box is told to the station as it changes, and empty is no call', async () => {
    await renderCockpit(base())
    fireEvent.change(toBox(), { target: { value: 'w1aw' } })
    await settle()
    expect(spies.js8SelectCall).toHaveBeenLastCalledWith('W1AW')
    fireEvent.change(toBox(), { target: { value: '' } })
    await settle()
    expect(spies.js8SelectCall).toHaveBeenLastCalledWith(null)
  })

  it('turning HB on clears the To box with the QSO pause on, and keeps it with the pause off', async () => {
    for (const [qsoPause, left] of [[true, ''], [false, 'W1AW']] as const) {
      await renderCockpit(base({ hbIntervalMin: 5, presence: presence({ qsoPause }) }))
      fireEvent.change(toBox(), { target: { value: 'W1AW' } })
      fireEvent.click(q('.js8-hb')!)
      await settle()
      expect(spies.js8Arm).toHaveBeenLastCalledWith('hb', true)
      expect(toBox().value, `QSO pause ${qsoPause}`).toBe(left)
      cleanup()
    }
  })

  it('a repeat the QSO pause holds says so on HB', async () => {
    await renderCockpit(base({ hbIntervalMin: 5, presence: presence({ hbPaused: true }) }))
    expect(q('.js8-hb')!.title).toBe(t('js8.dock.hb.paused'))
    expect(q('.js8-hb')!.getAttribute('aria-pressed')).toBe('false')
  })
})

describe('HB ACK: held back now, and the 55-minute rule', () => {
  const armedAck = (over: Partial<Js8Presence>, speed: Js8State['speed'] = 'normal') =>
    base({
      speed,
      hbAck: true,
      armed: { autoreply: true, relay: true, hbAck: true, hb: false, cq: false },
      presence: presence(over),
    })

  it('armed and nothing holds it back: it reads armed', async () => {
    await renderCockpit(armedAck({}))
    expect(q('.js8-hback')!.classList.contains('armed')).toBe(true)
    expect(q('.js8-hback')!.title).toBe(t('js8.dock.hbAck.title.armed') + differs)
  })

  it.each([
    ['qsoPause', 'normal', () => t('js8.dock.hbAck.suppressed.qsoPause')],
    ['arriving', 'normal', () => t('js8.dock.hbAck.suppressed.arriving')],
    ['speed', 'turbo', () => t('js8.dock.hbAck.suppressed.speed', { speed: 'JS8 40' })],
  ] as const)('held back by %s: on but not armed, and it says why', async (why, speed, words) => {
    await renderCockpit(armedAck({ hbAckSuppressed: why }, speed))
    const ack = q('.js8-hback')!
    expect([ack.classList.contains('on'), ack.classList.contains('armed')]).toEqual([true, false])
    expect(ack.title).toBe(words() + differs)
  })

  it('the stations the 55-minute rule stopped answering are named on it', async () => {
    await renderCockpit(armedAck({ hbAckRateBlocked: ['W1AW', 'K1ABC'] }))
    expect(q('.js8-hback')!.title).toBe(
      `${t('js8.dock.hbAck.title.armed')} ${t('js8.dock.hbAck.rateBlocked', { calls: 'W1AW, K1ABC' })}` + differs,
    )
  })
})

describe('not taking part in @ALLCALL', () => {
  it('@ALLCALL leaves the To box’s list, and is in it otherwise', async () => {
    const options = () => Array.from(document.querySelectorAll('#js8-to-list option')).map((o) => (o as HTMLOptionElement).value)
    await renderCockpit(base())
    expect(options()).toContain('@ALLCALL')
    cleanup()
    await renderCockpit(base({ presence: presence({ avoidAllcall: true }) }))
    expect(options()).not.toContain('@ALLCALL')
  })
})
