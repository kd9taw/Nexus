// @vitest-environment jsdom
//
// THE TYPEAHEAD IN JS8CALL 3.0'S COMPOSE BOX. While a message of the box's goes out, JS8Call keeps
// typing in the box (mainwindow.cpp:3589-3668), adds no other text to it (`addMessageText`,
// :3288-3294, "don't add message text if we already have a transmission queued") and greys out
// REPLY, SNR, INFO, STATUS and its saved messages (`updateButtonDisplay`, :5675-5696). This file
// holds the dock's compose box (Js8ComposeBox, with its pills and underlined words) to that,
// against a mocked engine: a MSG going out keeps its locked title over a pill's, a button that fills
// the box adds nothing, the one-click buttons are off until the message has gone, and a word picked
// in place of an underlined one goes to the engine like typing — never into a read-only box.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor, within } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import type { AppSnapshot, Js8InboxEntry, Js8State, Js8Typeahead } from '../types'
import { t } from '../i18n'
import { JS8_COMPOSE_DEFAULTS, type Js8ComposeConfig } from '../js8Compose'

// THE BUDGET: the house's 15 s (a loaded full suite runs cases many times slower than one core).
vi.setConfig({ testTimeout: 15_000 })

const base = (): Js8State => ({
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
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [
    { atMs: Date.now() - 30_000, speed: 'normal', freqHz: 1500, snrDb: -3, dtS: -0.12, from: 'W1AW', text: 'W1AW: KD9TAW SNR?', directedToMe: true, mine: false, complete: true, lowConf: false },
  ],
  stations: [{ call: 'W1AW', grid: 'FN31', snrDb: -3, freqHz: 1500, speed: 'normal', lastMs: Date.now(), lastHb: false, lastCq: false, storedMsgs: 0 }],
  inbox: engine.inbox,
  queue: [],
  pendingReply: null,
  lastError: null,
})

/** What the engine says of the box's message: `null` while none goes out. And its Inbox. */
const engine = vi.hoisted(() => ({ typeahead: null as Js8Typeahead | null, inbox: [] as Js8InboxEntry[] }))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getJs8State: vi.fn(async () => base()),
    js8Enter: vi.fn(async () => base()),
    js8Send: vi.fn(async () => base()),
    js8SendCommand: vi.fn(async () => base()),
    js8CallCq: vi.fn(async () => base()),
    js8Composer: vi.fn(async () => null),
    js8LocatorRefusal: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
    js8TypeaheadState: vi.fn(async () => engine.typeahead),
    js8Typeahead: vi.fn(async () => engine.typeahead),
    js8WordCheck: vi.fn(async (words: string[]) => words.map((w) => w === 'HELLOX')),
    js8WordSuggestions: vi.fn(async () => ({ words: ['HELLO', 'HELLOS'], found: true })),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))

const js8Send = api.js8Send as ReturnType<typeof vi.fn>
const js8CallCq = api.js8CallCq as ReturnType<typeof vi.fn>
const js8Typeahead = api.js8Typeahead as ReturnType<typeof vi.fn>
const js8WordSuggestions = api.js8WordSuggestions as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52hw',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  engine.typeahead = null
  engine.inbox = []
  for (const f of [js8Send, js8CallCq, js8Typeahead, js8WordSuggestions]) f.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(compose: Partial<Js8ComposeConfig> = {}) {
  const r = render(<Js8Cockpit snap={snap} compose={{ ...JS8_COMPOSE_DEFAULTS, ...compose }} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}
const q = <T extends Element>(sel: string) => document.querySelector(sel) as T
const box = () => q<HTMLInputElement>('.js8-compose')
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const click = async (el: Element) => {
  await act(async () => {
    fireEvent.click(el)
  })
}

/** Type `text` and press Enter, the engine taking it as `taken`; the box then holds the message. */
async function send(text: string, taken: Js8Typeahead) {
  type(box(), text)
  engine.typeahead = taken
  await act(async () => {
    fireEvent.keyDown(box(), { key: 'Enter' })
  })
  await waitFor(() => expect(box().value).toBe(taken.unsent))
}

/** The message has gone: the cockpit's poll empties the box. */
async function gone() {
  engine.typeahead = null
  await waitFor(() => expect(box().value).toBe(''), { timeout: 2000 })
}

/** Let the word check's pause run out and its answer come in. */
async function wordCheck() {
  await act(async () => {
    vi.advanceTimersByTime(200)
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('the compose box while its message goes out (typeahead)', () => {
  it("a MSG going out keeps its locked title over a pill's", async () => {
    await renderCockpit()
    type(box(), 'W1AW MSG HI')
    fireEvent.mouseMove(box(), { clientX: 0, clientY: 0 })
    expect(box().title, 'control: the pill under the pointer titles the box').toBe(t('js8.pill.recipient', { call: 'W1AW' }))
    await send('W1AW MSG HI', { sent: '', unsent: 'KD9TAW: W1AW MSG HI', locked: true })
    expect(js8Send).toHaveBeenCalledWith(null, 'W1AW MSG HI')
    expect(q('.js8-pill.sender'), 'premise: a pill still lies under the pointer').not.toBeNull()
    fireEvent.mouseMove(box(), { clientX: 0, clientY: 0 })
    expect(box().title).toBe(t('js8.dock.typeahead.locked'))
    expect(box().readOnly).toBe(true)
  })

  it('REPLY, SNR, INFO, STATUS and Saved are off while it goes out, and on again once it has gone', async () => {
    await renderCockpit({ info: 'QTH <MYGRID4>' })
    type(q<HTMLInputElement>('.js8-to'), 'W1AW')
    const off = () => ['.js8-reply', '.js8-snr-send', '.js8-info', '.js8-status', '.js8-saved'].map((s) => q<HTMLButtonElement>(s).disabled)
    expect(off(), 'control: W1AW heard and INFO set, each is on').toEqual([false, false, false, false, false])
    await send('HELLO', { sent: '', unsent: 'KD9TAW: W1AW HELLO', locked: false })
    expect(js8Send).toHaveBeenCalledWith('W1AW', 'HELLO')
    expect(off(), 'while it goes out').toEqual([true, true, true, true, true])
    await gone()
    expect(off(), 'once it has gone').toEqual([false, false, false, false, false])
  })

  it('a button that fills the box adds nothing while it goes out, as JS8Call adds no text then', async () => {
    await renderCockpit({ sendAtOnce: false })
    await send('HELLO', { sent: '', unsent: 'KD9TAW: HELLO', locked: false })
    await click(q('.js8-cq'))
    expect(box().value, 'the message going out, unchanged').toBe('KD9TAW: HELLO')
    expect(js8Typeahead, 'nothing handed to the engine').not.toHaveBeenCalled()
    expect(js8CallCq, 'and nothing sent').not.toHaveBeenCalled()
    await gone()
    await click(q('.js8-cq'))
    expect(box().value, 'control: once it has gone, CQ fills the box').toBe('CQ CQ CQ EN52')
  })

  it('the Inbox Reply fills nothing while it goes out, and fills the reply once it has gone', async () => {
    // A message to me, already read, so picking it asks nothing of the engine.
    engine.inbox = [{ id: 1, from: 'W0IND', to: 'KD9TAW', text: 'HELLO', path: ['W0IND'], state: 'read', atMs: Date.now() - 60_000, freqHz: 1503, snrDb: -8 }]
    await renderCockpit()
    await waitFor(() => expect(q('[data-pane="inbox"] .js8-inbox-row')).not.toBeNull())
    await click(q('[data-pane="inbox"] .js8-inbox-row'))
    await send('HELLO', { sent: '', unsent: 'KD9TAW: HELLO', locked: false })
    expect(q<HTMLButtonElement>('.js8-inbox-reply').disabled, 'premise: Reply can be pressed').toBe(false)
    await click(q('.js8-inbox-reply'))
    expect(box().value, 'the message going out, unchanged').toBe('KD9TAW: HELLO')
    expect(js8Typeahead, 'nothing handed to the engine').not.toHaveBeenCalled()
    await gone()
    await click(q('.js8-inbox-reply'))
    expect(box().value, 'control: once it has gone, Reply fills the box').toBe('W0IND MSG [MESSAGE]')
    // `[MESSAGE]` picked, so typing replaces it.
    await waitFor(() => expect([box().selectionStart, box().selectionEnd]).toEqual([10, 19]))
    expect(js8Send, 'Reply sends nothing').toHaveBeenCalledTimes(1)
  })

  it('a word picked for an underlined one goes to the engine as typing does', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      await renderCockpit({ pills: false })
      await send('HELLOX THERE', { sent: '', unsent: 'KD9TAW: HELLOX THERE', locked: false })
      await wordCheck()
      expect([...document.querySelectorAll('.js8-word-flag')].map((f) => f.textContent)).toEqual(['HELLOX'])
      await act(async () => {
        fireEvent.contextMenu(box(), { clientX: 0, clientY: 0 })
        await Promise.resolve()
      })
      expect(js8WordSuggestions).toHaveBeenCalledWith('HELLOX')
      await act(async () => {
        await Promise.resolve()
      })
      await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'HELLO' }))
      expect(box().value).toBe('KD9TAW: HELLO THERE')
      expect(js8Typeahead).toHaveBeenLastCalledWith(0, 'KD9TAW: HELLO THERE')
    } finally {
      vi.useRealTimers()
    }
  })

  it('a read-only box (a MSG going out) offers no word in place of an underlined one', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    try {
      await renderCockpit({ pills: false })
      await send('W1AW MSG HELLOX', { sent: '', unsent: 'KD9TAW: W1AW MSG HELLOX', locked: true })
      await wordCheck()
      expect([...document.querySelectorAll('.js8-word-flag')].map((f) => f.textContent), 'premise: underlined').toEqual(['HELLOX'])
      expect(box().readOnly, 'premise: read-only').toBe(true)
      await act(async () => {
        fireEvent.contextMenu(box(), { clientX: 0, clientY: 0 })
        await Promise.resolve()
      })
      expect(js8WordSuggestions, 'nothing asked').not.toHaveBeenCalled()
      expect(screen.queryByRole('menu'), 'no menu').toBeNull()
      expect(box().value).toBe('KD9TAW: W1AW MSG HELLOX')
    } finally {
      vi.useRealTimers()
    }
  })
})

// THE RX PANE'S CLEAR, as JS8Call's `clearRXActivity` (mainwindow.cpp:3001-3014): it empties the
// compose box as well as the pane, and so does "Clear All Lists", which runs it (:2970-2990); the
// other two lists' Clear leave the box alone (:2993-2999, :3016-3031). JS8Call's box holds the whole
// message, so here the text and the command go; the To box is JS8Call's selected call, which stays.
// While the box's own message goes out it holds what is still to go: that is left to go.
describe('the RX pane’s Clear empties the compose box, as JS8Call’s does', () => {
  /** Open a pane's ⋯ and pick an entry. */
  async function pick(pane: string, entry: string) {
    const trigger = q(`[data-pane="${pane}"] .pane-menu`)
    await act(async () => {
      fireEvent.pointerDown(trigger, { button: 0, pointerType: 'mouse' })
    })
    const item = await waitFor(() => {
      const hit = [...document.querySelectorAll('[role^="menuitem"]')].find((el) => el.textContent === entry)
      expect(hit, `no "${entry}" in the ${pane} menu`).toBeTruthy()
      return hit as HTMLElement
    })
    await act(async () => {
      fireEvent.click(item)
    })
  }
  const cmd = () => q<HTMLSelectElement>('.js8-cmd-select')
  const to = () => q<HTMLInputElement>('.js8-to')
  const write = () => {
    type(box(), 'HELLO')
    fireEvent.change(cmd(), { target: { value: cmd().options[1].value } })
  }

  it('RX’s Clear and Clear all lists empty the text and the command; Band activity’s and the call list’s Clear leave them', async () => {
    await renderCockpit()
    type(to(), 'W1AW')
    write()
    const picked = cmd().options[1].value
    await pick('offsets', t('js8.lists.clear'))
    await pick('stations', t('js8.lists.clear'))
    expect([box().value, cmd().value], 'Band activity’s and the call list’s Clear leave the box').toEqual(['HELLO', picked])
    await pick('activity', t('js8.lists.clear'))
    expect([box().value, cmd().value, to().value], 'RX’s Clear empties it; the selected call stays').toEqual(['', '', 'W1AW'])
    write()
    expect([box().value, cmd().value], 'premise: written again').toEqual(['HELLO', picked])
    await pick('activity', t('js8.lists.clearAll'))
    expect([box().value, cmd().value, to().value], 'Clear all lists empties it too').toEqual(['', '', 'W1AW'])
  })

  it('while the box’s own message goes out, Clear leaves it holding what is still to go and hands the engine nothing', async () => {
    await renderCockpit()
    await send('HELLO', { sent: '', unsent: 'KD9TAW: HELLO', locked: false })
    await pick('activity', t('js8.lists.clear'))
    await pick('activity', t('js8.lists.clearAll'))
    expect(box().value, 'the message going out, unchanged').toBe('KD9TAW: HELLO')
    expect(js8Typeahead, 'nothing handed to the engine').not.toHaveBeenCalled()
    await gone()
  })
})
