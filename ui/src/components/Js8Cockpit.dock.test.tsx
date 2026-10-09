// @vitest-environment jsdom
//
// THE JS8 TX DOCK — what each control asks the engine, and the three faces of a second-act
// chip. The invariant this file exists for (spec TX-safety 1 and 11): a switch that is ON
// while the session TX latch is OFF must never LOOK armed — the APRS rule — and a pending
// auto-reply is visible and asks Yes / No before it can go (JS8Call's confirmation box). Nothing here can key:
// every handler is an engine call, and the engine refuses on a receive-only tier (B6) or on
// a down gate (B7) with a reason this cockpit toasts.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import type { AppSnapshot, Js8State } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "never writes over what the operator typed, and says the…", takes
// 0.64 s and 0.63 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
  stations: [{ call: 'W1AW', grid: 'FN31', snrDb: -3, freqHz: 1500, speed: 'normal', lastMs: Date.now(), lastHb: true, lastCq: false, storedMsgs: 0 }],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: base() }

/** What the station offers the compose box (AUTO off), as `js8_composer` answers. */
const composer = vi.hoisted(() => ({ offer: null as { id: number; text: string } | null }))
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
    js8AnswerReply: vi.fn(async () => s()),
    js8Composer: vi.fn(async () => composer.offer),
    js8DropQueue: vi.fn(async () => s()),
    js8LocatorRefusal: vi.fn(async () => null),
    // The roster's ✓/Name/Comment columns join against the logbook (features/callHistory),
    // so the auto-stub's `{}` is not a usable log — this suite runs against an empty one.
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
const js8AnswerReply = api.js8AnswerReply as ReturnType<typeof vi.fn>
const js8Composer = api.js8Composer as ReturnType<typeof vi.fn>
const js8DropQueue = api.js8DropQueue as ReturnType<typeof vi.fn>
const js8LocatorRefusal = api.js8LocatorRefusal as ReturnType<typeof vi.fn>

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
  js8AnswerReply.mockClear()
  js8Composer.mockClear()
  composer.offer = null
  js8DropQueue.mockClear()
  js8LocatorRefusal.mockReset()
  js8LocatorRefusal.mockImplementation(async () => null)
  toast.pushToast.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(s: AppSnapshot = snap) {
  const r = render(<Js8Cockpit snap={s} />)
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
    state.current = { ...base(), txEnabled: true, armed: { autoreply: true, relay: true, hbAck: false, hb: false, cq: false } }
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

// JS8Call's AutoreplyConfirmation (on by default, Configuration.cpp:1949): each automatic reply is a
// question, its own words (mainwindow.cpp:5211-5212), Yes / No with the seconds to No on No.
describe('the automatic reply asks Yes / No, as JS8Call does', () => {
  const asking = () => ({ origin: 'autoReply' as const, to: 'W1AW', display: 'KD9TAW: W1AW SNR -03', firesAtMs: Date.now() + 89_000 })

  it("asks in JS8Call's words, in the TX dock, with Yes and No (seconds) and no Cancel", async () => {
    state.current = { ...base(), txEnabled: true, pendingReply: asking() }
    await renderCockpit()
    const row = q('.js8-confirm-row')
    expect(row).not.toBeNull()
    expect(row.closest('.cockpit-txdock')).not.toBeNull()
    expect(row.closest('.pane-frame')).toBeNull()
    expect(row.textContent).toContain('A transmission is queued for autoreply: KD9TAW: W1AW SNR -03')
    expect(row.textContent).toContain('would you like to send this transmission?')
    expect(q('.js8-confirm-yes').textContent).toBe('Yes')
    expect(q('.js8-confirm-no').textContent).toMatch(/^No \((89|88)\)$/)
  })

  it('Yes and No each answer THAT reply, named by what it showed', async () => {
    const p = asking()
    state.current = { ...base(), txEnabled: true, pendingReply: p }
    await renderCockpit()
    await act(async () => {
      fireEvent.click(q('.js8-confirm-yes'))
    })
    expect(js8AnswerReply).toHaveBeenLastCalledWith(true, p.display, p.firesAtMs)
    await act(async () => {
      fireEvent.click(q('.js8-confirm-no'))
    })
    expect(js8AnswerReply).toHaveBeenLastCalledWith(false, p.display, p.firesAtMs)
  })

  it('with TX off it still asks, and says nothing will key', async () => {
    state.current = { ...base(), pendingReply: asking() }
    await renderCockpit()
    expect(q('.js8-confirm-row').textContent).toMatch(/TX is off, nothing keys/)
    expect(q('.js8-confirm-yes')).not.toBeNull()
    expect(q('.js8-confirm-no')).not.toBeNull()
  })
})

// With AUTO off JS8Call types the reply into its compose box for the operator to send
// (`addMessageText`, mainwindow.cpp:9671) and never keys it (:9674-9685); the box takes it only
// when it is empty (:9657).
describe('with AUTO off the reply lands in the compose box, for you to send', () => {
  it('fills an empty box with the reply, clears To and the command, and names it back', async () => {
    composer.offer = { id: 7, text: 'W1AW SNR -03' }
    await renderCockpit()
    type('.js8-to', 'K1ABC')
    const from = js8Composer.mock.calls.length
    type('.js8-compose', '')
    await waitFor(() => expect(q<HTMLInputElement>('.js8-compose').value).toBe('W1AW SNR -03'))
    expect(q<HTMLInputElement>('.js8-to').value, 'the reply names its station itself').toBe('')
    expect(q<HTMLSelectElement>('.js8-cmd-select').value, 'sent as typed, no command').toBe('')
    composer.offer = null
    // The sync after the fill tells the station the box holds reply 7, (true, 7); the next 500 ms poll then says
    // (true, null). So the LAST call is (true, 7) only until that poll, and load can bring the poll before this check:
    // the call is looked for among those made since the box was emptied.
    await waitFor(() => expect(js8Composer.mock.calls.slice(from)).toContainEqual([true, 7]))
    await act(async () => {
      fireEvent.click(q('.js8-send'))
    })
    expect(js8Send).toHaveBeenLastCalledWith(null, 'W1AW SNR -03')
  })

  it('a reply that arrives later, the box still empty, is picked up on the next poll', async () => {
    await renderCockpit()
    await waitFor(() => expect(js8Composer).toHaveBeenCalledWith(false, null))
    composer.offer = { id: 9, text: 'W1AW ACK' }
    await waitFor(() => expect(q<HTMLInputElement>('.js8-compose').value).toBe('W1AW ACK'), { timeout: 2000 })
  })

  it('never writes over what the operator typed, and says the box holds text', async () => {
    await renderCockpit()
    type('.js8-compose', 'HELLO')
    composer.offer = { id: 8, text: 'W1AW SNR -03' }
    await waitFor(() => expect(js8Composer).toHaveBeenLastCalledWith(true, null))
    await act(async () => {
      await new Promise((r) => setTimeout(r, 600))
    })
    expect(q<HTMLInputElement>('.js8-compose').value).toBe('HELLO')
    expect(js8Composer).not.toHaveBeenCalledWith(expect.anything(), 8)
  })
})

// JS8Call's query menu has "GRID <locator> - Send my current station Maidenhead grid locator"
// (mainwindow.cpp:6656), disabled with no locator (:6657), which sends `<call> GRID <my_grid()>`
// (:6665) at once (TransmitDirected, on by default: Configuration.cpp:1947). The station row's

// one-click queries are Nexus's query menu.
describe('send my grid, from a station row', () => {
  const sendGrid = () => q<HTMLButtonElement>('.js8-station-acts .js8-send-grid')
  /** The engine's JS8_NO_LOCATOR, the gate's words. */
  const NO_LOCATOR = 'Set your Maidenhead grid (e.g. EN52) in Settings before transmitting JS8.'

  it('one click sends that station GRID and the whole locator', async () => {
    await renderCockpit({ ...snap, mygrid: 'en52xa' } as AppSnapshot)
    expect(sendGrid()?.textContent).toBe('GRID EN52XA')
    expect(sendGrid().title).toBe('Send your grid square EN52XA to W1AW')
    await act(async () => {
      fireEvent.click(sendGrid())
    })
    expect(js8SendCommand).toHaveBeenCalledWith('W1AW', 15, 'EN52XA')
  })

  it('is disabled with no locator in Settings', async () => {
    js8LocatorRefusal.mockImplementation(async () => NO_LOCATOR)
    await renderCockpit({ ...snap, mygrid: '  ' } as AppSnapshot)
    expect(sendGrid()?.disabled).toBe(true)
    expect(sendGrid().textContent).toBe('GRID')
    await act(async () => {
      fireEvent.click(sendGrid())
    })
    expect(js8SendCommand).not.toHaveBeenCalled()
  })

  // JS8Call's Settings refuse a malformed locator (Configuration.cpp:2443-2446), so its GRID item
  // never meets one. Nexus's Settings can hold one, which the JS8 gate refuses; the button takes
  // the gate's own answer (`js8_locator_refusal`), not a copy of its rule.
  it("is disabled, with the gate's reason as its tooltip, whenever the JS8 gate refuses the locator", async () => {
    js8LocatorRefusal.mockImplementation(async () => NO_LOCATOR)
    await renderCockpit({ ...snap, mygrid: 'EN5' } as AppSnapshot)
    expect(sendGrid()?.disabled, 'a locator the gate refuses').toBe(true)
    expect(sendGrid().title, "the gate's reason").toBe(NO_LOCATOR)
    await act(async () => {
      fireEvent.click(sendGrid())
    })
    expect(js8SendCommand).not.toHaveBeenCalled()
  })

  it('asks the gate again when the locator in Settings changes', async () => {
    js8LocatorRefusal.mockImplementation(async () => NO_LOCATOR)
    const r = await renderCockpit({ ...snap, mygrid: 'EN5' } as AppSnapshot)
    expect(sendGrid()?.disabled, 'control: EN5 is refused').toBe(true)
    js8LocatorRefusal.mockImplementation(async () => null)
    r.rerender(<Js8Cockpit snap={{ ...snap, mygrid: 'EN52' } as AppSnapshot} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(sendGrid()?.disabled, 'EN52, which the gate takes').toBe(false)
    expect(sendGrid().title).toBe('Send your grid square EN52 to W1AW')
  })
})
