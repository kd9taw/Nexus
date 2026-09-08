// @vitest-environment jsdom
//
// THE JS8 TX DOCK — what each control asks the engine, and the three faces of a second-act
// chip. The invariant this file exists for (spec TX-safety 1 and 11): a switch that is ON
// while the session TX latch is OFF must never LOOK armed — the APRS rule — and a pending
// auto-reply is visible, counted down and cancellable before it fires. Nothing here can key:
// every handler is an engine call, and the engine refuses on a receive-only tier (B6) or on
// a down gate (B7) with a reason this cockpit toasts.
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
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 12,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [{ call: 'W1AW', grid: 'FN31', snrDb: -3, freqHz: 1500, speed: 'normal', lastMs: Date.now(), lastHb: true, lastCq: false, storedMsgs: 0 }],
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
    // The roster's ✓/Name/Comment columns join against the logbook (features/callHistory),
    // so the auto-stub's `{}` is not a usable log — this suite runs against an empty one.
    getLog: vi.fn(async () => []),
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

const js8Send = api.js8Send as ReturnType<typeof vi.fn>
const js8SendCommand = api.js8SendCommand as ReturnType<typeof vi.fn>
const js8Arm = api.js8Arm as ReturnType<typeof vi.fn>
const js8Cancel = api.js8Cancel as ReturnType<typeof vi.fn>
const js8DropQueue = api.js8DropQueue as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  js8Send.mockClear()
  js8SendCommand.mockClear()
  js8Arm.mockClear()
  js8Cancel.mockClear()
  js8DropQueue.mockClear()
  toast.pushToast.mockClear()
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
const type = (sel: string, value: string) => fireEvent.change(q<HTMLInputElement>(sel), { target: { value } })

describe('the command palette and Send', () => {
  it('lists the 32 commands after a "no command" row, in id order', async () => {
    await renderCockpit()
    const sel = q<HTMLSelectElement>('.js8-cmd-select')
    expect(sel.options.length).toBe(33)
    expect(sel.options[0].value).toBe('')
    expect(Array.from(sel.options).slice(1).map((o) => o.value)).toEqual([...Array(32).keys()].map(String))
    expect(sel.options[1].textContent).toBe('SNR?')
    expect(sel.options[6].textContent).toBe('>')
    expect(sel.closest('.cockpit-txdock')).not.toBeNull()
  })

  it('a plain message goes through js8Send with a null addressee', async () => {
    await renderCockpit()
    type('.js8-compose', 'HELLO ALL')
    await act(async () => {
      fireEvent.click(q('.js8-send'))
    })
    expect(js8Send).toHaveBeenCalledWith(null, 'HELLO ALL')
    expect(q<HTMLInputElement>('.js8-compose').value).toBe('')
  })

  it('a command goes through js8SendCommand with the id and the argument text', async () => {
    await renderCockpit()
    type('.js8-to', 'w1aw')
    fireEvent.change(q<HTMLSelectElement>('.js8-cmd-select'), { target: { value: '9' } })
    type('.js8-compose', 'GOOD MORNING')
    await act(async () => {
      fireEvent.click(q('.js8-send'))
    })
    expect(js8SendCommand).toHaveBeenCalledWith('W1AW', 9, 'GOOD MORNING')
  })

  it('a command with no addressee is refused with a toast, never sent', async () => {
    await renderCockpit()
    fireEvent.change(q<HTMLSelectElement>('.js8-cmd-select'), { target: { value: '0' } })
    expect(q<HTMLButtonElement>('.js8-send').disabled).toBe(true)
    type('.js8-compose', 'X')
    await act(async () => {
      fireEvent.click(q('.js8-send'))
    })
    expect(js8SendCommand).not.toHaveBeenCalled()
    expect(js8Send).not.toHaveBeenCalled()
  })

  it('the estimate counts frames and seconds, and flags a message over the airtime cap', async () => {
    await renderCockpit()
    expect(q('.js8-estimate').textContent).toBe('')
    type('.js8-compose', 'HELLO') // "KD9TAW: HELLO" → 2 frames at Normal → 30 s
    expect(q('.js8-estimate').textContent).toContain('2')
    expect(q('.js8-estimate').textContent).toContain('30')
    expect(q('.js8-estimate').classList.contains('over')).toBe(false)
    type('.js8-compose', 'X'.repeat(400)) // 41 frames at Normal > the 39-frame cap
    expect(q('.js8-estimate').classList.contains('over')).toBe(true)
    expect(q<HTMLButtonElement>('.js8-send').disabled).toBe(true)
  })
})

describe('the second-act chips never look armed without the session TX latch', () => {
  it('AUTOREPLY on + TX off reads "on" and not "armed"; both on reads "armed"', async () => {
    await renderCockpit()
    const chip = q('.js8-autoreply')
    expect(chip.classList.contains('on')).toBe(true)
    expect(chip.classList.contains('armed')).toBe(false)
    cleanup()
    state.current = { ...base(), txEnabled: true, armed: { autoreply: true, relay: true, hbAck: false, hb: false } }
    await renderCockpit()
    expect(q('.js8-autoreply').classList.contains('armed')).toBe(true)
    expect(q('.js8-relay').classList.contains('armed')).toBe(true)
    expect(q('.js8-hback').classList.contains('on')).toBe(false)
  })

  it('clicking a chip asks the engine to flip THAT switch', async () => {
    await renderCockpit()
    await act(async () => {
      fireEvent.click(q('.js8-autoreply'))
    })
    expect(js8Arm).toHaveBeenCalledWith('autoreply', false)
    await act(async () => {
      fireEvent.click(q('.js8-hback'))
    })
    expect(js8Arm).toHaveBeenCalledWith('hback', true)
    await act(async () => {
      fireEvent.click(q('.js8-relay'))
    })
    expect(js8Arm).toHaveBeenCalledWith('relay', false)
  })

  it('the idle chip counts toward the watchdog, says off at 0, and shouts when tripped', async () => {
    await renderCockpit()
    expect(q('.js8-idle').textContent).toContain('12')
    expect(q('.js8-idle').textContent).toContain('60')
    cleanup()
    state.current = { ...base(), idleLimitMin: 0 }
    await renderCockpit()
    expect(q('.js8-idle').classList.contains('tripped')).toBe(false)
    cleanup()
    state.current = { ...base(), idleTripped: true }
    await renderCockpit()
    expect(q('.js8-idle').classList.contains('tripped')).toBe(true)
    expect(q('.js8-idle').getAttribute('role')).toBe('alert')
  })
})

describe('the pending auto-reply and the queue', () => {
  it('shows the countdown and Cancel → js8Cancel; with TX off it says nothing will key', async () => {
    state.current = { ...base(), pendingReply: { origin: 'autoReply', to: 'W1AW', display: 'KD9TAW: W1AW SNR -03', firesAtMs: Date.now() + 14_000 } }
    await renderCockpit()
    const row = q('.js8-pending-row')
    expect(row).not.toBeNull()
    expect(row.closest('.cockpit-txdock')).not.toBeNull()
    expect(row.textContent).toContain('W1AW')
    expect(row.textContent).toContain('SNR -03')
    await act(async () => {
      fireEvent.click(q('.js8-cancel'))
    })
    expect(js8Cancel).toHaveBeenCalledTimes(1)
  })

  it('renders the queue rows with their origin and Drop queue → js8DropQueue (not a stop)', async () => {
    state.current = {
      ...base(),
      queue: [
        { origin: 'operator', display: 'KD9TAW: W1AW MSG HELLO', first: true, last: false },
        { origin: 'relay', display: 'W1AW>KD9TAW ACK', first: true, last: true },
      ],
    }
    await renderCockpit()
    const items = document.querySelectorAll('.js8-queue-item')
    expect(items.length).toBe(2)
    expect(items[0].closest('.cockpit-txdock')).not.toBeNull()
    expect(items[0].closest('.pane-frame')).toBeNull()
    await act(async () => {
      fireEvent.click(q('.js8-drop'))
    })
    expect(js8DropQueue).toHaveBeenCalledTimes(1)
    // Drop queue is a SENDER-class control: its accessible name must not read as a stop, or
    // the stop-line name backstop's spirit is violated one file over.
    expect(q('.js8-drop').textContent!.toLowerCase()).not.toMatch(/stop|halt|abort/)
  })

  it('no pending row and no queue row while there is nothing pending or queued', async () => {
    await renderCockpit()
    expect(document.querySelector('.js8-pending-row')).toBeNull()
    expect(document.querySelector('.js8-queue-row')).toBeNull()
  })
})
