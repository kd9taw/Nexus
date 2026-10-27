// @vitest-environment jsdom
//
// THE CW TURN CUE IN THE COCKPIT: a lamp first on the dock's send row saying whose turn it is, the
// F-key that fits lit, and the other station's speed beside ours. For a decoder user: only with the
// Decode pane shown and the AI decoder on, never in an unassisted entry, and one ⊞ tick hides it for
// good. A screen reader hears a turn once per over, and the opt-in sound plays once per over, both
// never while anything is keyed.
//
// ⛔ IT NEVER SENDS. No state it shows and no click on it reaches a send: the F-keys send only
// when pressed, exactly as without it.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, CwDecodeResult, CwTurn, FieldDayStatus } from '../types'
import type { CwPanelId, PanelLayoutApi } from '../features/panelState'
import { EN } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { get, spies }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
const heard = vi.hoisted(() => ({ turnEarcon: vi.fn(), announce: vi.fn() }))
vi.mock('../alerts', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  turnEarcon: heard.turnEarcon,
}))
vi.mock('../announce', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  announce: heard.announce,
}))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: ReactNode }) => <header className="cockpit-header">{children}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** What the station's next read says. */
let read: Partial<CwDecodeResult> = {}
beforeEach(() => {
  read = {}
  heard.turnEarcon.mockClear()
  heard.announce.mockClear()
  for (const spy of Object.values(api.spies)) spy.mockClear()
  api.get('getCatCwUnprovenRigModels').mockImplementation(async () => [])
  api.get('cwDecode').mockImplementation(async () => ({
    text: '', pending: '', wpm: 0, sent: [], keyerError: null, candidates: [], rst: null, name: null,
    workedCall: null, ...read,
  }))
  api.get('previewCw').mockImplementation(async (text: string) => text)
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function snap(over: { ai?: boolean; transmitting?: boolean } = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    aiCw: { enabled: over.ai ?? true, status: '', text: '' },
    radio: {
      dialMhz: 14.025, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW',
      transmitting: over.transmitting ?? false, txEnabled: true, txAllowed: true, cwWpm: 25, cwKeyer: 'cat',
    },
  } as unknown as AppSnapshot
}

function panels(removed: CwPanelId[] = []): PanelLayoutApi<CwPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

const turn = (t: Partial<CwTurn>): CwTurn => ({
  state: 'idle', role: null, hint: null, call: null, over: null, window: null, ...t,
})
const YOUR_TURN = turn({ state: 'yourTurn', role: 'answer', call: 'W1ABC', over: 4000 })

/** Let a send that waits on the His Call commit reach the api, as it would after a click. */
const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
/** Let the decode poll (every 200 ms) read the station. */
const nextPoll = async () => {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 260))
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}
const cockpit = (props: Partial<Parameters<typeof CwCockpit>[0]> = {}) => (
  <CwCockpit snap={snap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />
)
const lampEl = () => document.querySelector('.cw-send .cw-turn') as HTMLElement | null
const key = (k: string) => document.querySelector(`[data-cw-macro="${k}"] .cw-macro`) as HTMLButtonElement
const cued = () =>
  [...document.querySelectorAll('.cw-macro')]
    .filter((b) => /\bturn-(now|soon|maybe)\b/.test(b.className))
    .map((b) => `${b.closest('[data-cw-macro]')?.getAttribute('data-cw-macro')}:${b.className.match(/turn-(\w+)/)![1]}`)
const pace = () => document.querySelector('.cw-pace') as HTMLButtonElement | null
const SENDS = ['sendCw', 'sendCwArmed']
const sends = () => SENDS.flatMap((n) => api.get(n).mock.calls)

describe('the turn cue on the CW screen', () => {
  it('leads the send row, and lights the key that fits', async () => {
    read = { turn: YOUR_TURN, wpm: 18 }
    render(cockpit())
    await nextPoll()
    const lamp = lampEl()
    expect(lamp, 'the lamp').not.toBeNull()
    expect(document.querySelector('.cw-send')!.firstElementChild, 'first on the send row').toBe(lamp)
    expect(lamp!.querySelector('.cw-turn-label')!.textContent).toBe('Your turn · F2')
    expect(lamp!.dataset.turn).toBe('yourTurn')
    expect(cued()).toEqual(['F2:now'])
    // The lit key is described as suggested, for a screen reader, and keeps its own name.
    const describedBy = key('F2').getAttribute('aria-describedby')
    expect(document.getElementById(describedBy!)?.textContent).toBe(EN['cw.turn.suggested'])
    expect(key('F1').getAttribute('aria-describedby')).toBeNull()
  })

  it('lights each state the way it means: dashed as the over ends, dotted when only a suggestion, none for 5NN TU', async () => {
    render(cockpit())
    for (const [t, label, lit] of [
      [turn({ state: 'finishing', role: 'answer', call: 'W1ABC', over: 1 }), 'W1ABC finishing…', ['F2:soon']],
      [turn({ state: 'unsure', role: 'again', call: 'W1ABC', over: 1 }), 'Not sure · F7', ['F7:maybe']],
      [turn({ state: 'hold', call: 'W2XYZ', over: 2 }), 'Hold · W2XYZ', []],
      [turn({ state: 'closing', role: 'close', call: 'W1ABC', over: 3 }), EN['cw.turn.closing'], ['F4:now']],
      [turn({ state: 'yourTurn', hint: 'fiveNnTu', call: 'VP8A', over: 4 }), 'Your turn · send 5NN TU', []],
    ] as [CwTurn, string, string[]][]) {
      read = { turn: t }
      await nextPoll()
      expect([lampEl()?.querySelector('.cw-turn-label')?.textContent, cued()], t.state).toEqual([label, lit])
    }
    expect(lampEl()!.title, 'the DX coming back: the tooltip says why not the Reply key').toContain('Send 5NN TU')
  })

  it('a DX coming back in a pileup: the lamp says to send 5NN TU, no key glows, and nothing is sent', async () => {
    read = { turn: turn({ state: 'yourTurn', hint: 'fiveNnTu', call: 'VP8A', over: 4 }), wpm: 18 }
    render(cockpit())
    await nextPoll()
    await nextPoll()
    await settle()
    expect({
      label: lampEl()?.querySelector('.cw-turn-label')?.textContent,
      lit: cued(),
      sent: sends(),
    }).toEqual({ label: 'Your turn · send 5NN TU', lit: [], sent: [] })
    // Control: the Reply key still sends when pressed, as without the cue.
    fireEvent.click(key('F3'))
    await settle()
    expect(api.get('sendCw').mock.calls.length).toBe(1)
  })

  it('shows only with the Decode pane shown and the AI decoder on, never unassisted, and its ⊞ tick hides it', async () => {
    const hidden = [
      ['the Decode pane closed', () => cockpit({ panels: panels(['decode']) }), {}],
      ['the AI decoder off', () => cockpit({ snap: snap({ ai: false }) }), {}],
      ['its ⊞ tick off', () => cockpit({ panels: panels(['turnCue']) }), {}],
      // A read that says unassisted hides it, even from a station that sent a turn as well.
      ['an unassisted entry', () => cockpit(), { unassisted: true }],
    ] as const
    for (const [why, node, extra] of hidden) {
      read = { turn: YOUR_TURN, wpm: 18, ...extra }
      render(node())
      await nextPoll()
      expect([lampEl(), cued(), pace()], why).toEqual([null, [], null])
      cleanup()
    }
    // Control: every condition met, the same read shows it.
    read = { turn: YOUR_TURN, wpm: 18 }
    render(cockpit({ panels: panels() }))
    await nextPoll()
    expect([lampEl() != null, cued(), pace() != null]).toEqual([true, ['F2:now'], true])
  })

  it('with Enter Sends Message running it says only when, and ESM lights its own keys', async () => {
    read = { turn: turn({ state: 'yourTurn', role: 'myCall', call: 'N9XX', over: 5 }) }
    render(cockpit({ fieldDay: CONTEST, esmSetting: { on: true, callOnce: false, onSwitch: () => {} } }))
    await nextPoll()
    expect(lampEl()!.querySelector('.cw-turn-label')!.textContent).toBe('Your turn · Enter')
    expect(cued()).toEqual([])
  })
})

describe('the turn cue never sends', () => {
  it('through every state it shows, and from its pace hint, nothing is sent', async () => {
    render(cockpit())
    for (const t of [
      turn({ state: 'theySend', call: 'W1ABC', over: 1 }),
      turn({ state: 'finishing', role: 'answer', call: 'W1ABC', over: 1 }),
      YOUR_TURN,
      turn({ state: 'yourTurn', role: 'myCall', hint: 'partial', over: 6 }),
      turn({ state: 'closing', role: 'close', over: 7 }),
      turn({ state: 'unsure', role: 'again', over: 8 }),
      turn({ state: 'idle', role: 'cq', hint: 'noReply' }),
      turn({ state: 'listen', window: { total: 8, left: 5 } }),
      turn({ state: 'hold', hint: 'splitOff', over: 9 }),
    ]) {
      read = { turn: t, wpm: 18 }
      await nextPoll()
    }
    await settle()
    expect(sends(), 'a cue sent something').toEqual([])
    // The pace hint keys at their speed for this contact, the saved speed untouched; it sends nothing.
    fireEvent.click(pace()!)
    await settle()
    expect(api.get('setCwWpm').mock.calls).toEqual([[18, false]])
    expect(pace()!.textContent).toBe('them 18 · you 18 WPM')
    expect(sends(), 'the pace click sent something').toEqual([])
  })

  it('control: the lit key still sends when pressed, as without the cue', async () => {
    read = { turn: YOUR_TURN }
    render(cockpit())
    await nextPoll()
    fireEvent.click(key('F2'))
    await settle()
    expect(api.get('sendCw').mock.calls.length).toBe(1)
  })
})

describe('what a screen reader hears, and the sound', () => {
  it('a turn is said once per over, and again for the next over', async () => {
    render(cockpit())
    read = { turn: YOUR_TURN }
    await nextPoll()
    await nextPoll()
    expect(heard.announce.mock.calls).toEqual([['Your turn: F2.']])
    // Keying heard again for a moment takes the turn back, then gives it again: the same over.
    read = { turn: { ...YOUR_TURN, state: 'theySend', role: null } }
    await nextPoll()
    read = { turn: YOUR_TURN }
    await nextPoll()
    expect(heard.announce.mock.calls.length, 'the same over, said once').toBe(1)
    read = { turn: { ...YOUR_TURN, over: 9000 } }
    await nextPoll()
    expect(heard.announce.mock.calls.length).toBe(2)
  })

  it('never while anything is keyed', async () => {
    read = { turn: YOUR_TURN }
    render(cockpit({ snap: snap({ transmitting: true }), turnSound: true }))
    await nextPoll()
    expect([heard.announce.mock.calls, heard.turnEarcon.mock.calls]).toEqual([[], []])
  })

  it('"not sure" only at the fullest setting, and nothing at all with announcements off', async () => {
    read = { turn: turn({ state: 'unsure', role: 'again', over: 1 }) }
    render(cockpit())
    await nextPoll()
    expect(heard.announce.mock.calls, 'the default setting').toEqual([])
    cleanup()
    render(cockpit({ announceVerbosity: 'all' }))
    await nextPoll()
    expect(heard.announce.mock.calls).toEqual([[EN['cw.turn.say.unsure']]])
    cleanup()
    heard.announce.mockClear()
    read = { turn: YOUR_TURN }
    render(cockpit({ announceVerbosity: 'off' }))
    await nextPoll()
    expect(heard.announce.mock.calls).toEqual([])
  })

  it('the sound plays only when chosen, once per over, on your turn', async () => {
    read = { turn: YOUR_TURN }
    render(cockpit())
    await nextPoll()
    expect(heard.turnEarcon.mock.calls.length, 'off as it ships').toBe(0)
    cleanup()
    render(cockpit({ turnSound: true }))
    await nextPoll()
    await nextPoll()
    expect(heard.turnEarcon.mock.calls.length).toBe(1)
    read = { turn: turn({ state: 'hold', call: 'W2XYZ', over: 9000 }) }
    await nextPoll()
    expect(heard.turnEarcon.mock.calls.length, 'a hold makes no sound').toBe(1)
  })
})

/** A contest running, for the contest sets (the ILQP: an exchange with a report). */
const CONTEST = {
  running: true,
  state: 'Idle',
  event: 'ilqp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  role: 'in_state',
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
  ],
  sentExchange: 'COOK',
} as unknown as FieldDayStatus
