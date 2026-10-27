// @vitest-environment jsdom
//
// THE VOICE KEYER'S F-KEYS FROM THE LOG LINE — the REAL Phone cockpit, voice keyer and log line. A
// recorded slot's key plays it with the cursor in the Call box or any other box of the log line, as
// CW's macros send from anywhere in the CW cockpit (the operator, 2026-10-11), and F1–F12 are all
// held there, so none reaches the webview, where F5 reloads the app on Windows. A letter still types,
// a key with Alt, Ctrl or Cmd is still the system's, nothing plays while recording, Esc still stops,
// and Enter Sends Message steps on a key pressed in the box exactly as on one pressed outside it.
// Without station control, as on the hosted page, the keyer is not there and holds nothing.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot, FieldDayStatus } from '../types'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

vi.setConfig({ testTimeout: 30_000 })

const { SLOTS, playVoiceMessage, stopVoice, startVoiceRecording, contestLogManual } = vi.hoisted(() => ({
  // A station after the update that ran ESM on the built-in steps: CQ, My Call, Report and QRZ? on
  // F1–F4, AGN on F8, and F5 and F6 not recorded.
  SLOTS: [
    { slot: 1, label: 'CQ', file: '/voice/slot1.wav' },
    { slot: 2, label: 'My Call', file: '/voice/slot2.wav' },
    { slot: 3, label: 'Report', file: '/voice/slot3.wav' },
    { slot: 4, label: 'QRZ?', file: '/voice/slot4.wav' },
    { slot: 5, label: '73', file: '' },
    { slot: 6, label: 'Again', file: '' },
    { slot: 8, label: 'AGN', file: '/voice/slot8.wav' },
  ],
  playVoiceMessage: vi.fn(async (_slot: number) => ({})),
  stopVoice: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  contestLogManual: vi.fn(async (..._args: unknown[]) => ({})),
}))

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  getVoiceMessages: vi.fn(async () => SLOTS),
  playVoiceMessage,
  stopVoice,
  startVoiceRecording,
  // Saving the take on F6 binds it, and every other slot stays as it was.
  stopVoiceRecording: vi.fn(async () => SLOTS.map((m) => (m.slot === 6 ? { ...m, file: '/voice/slot6.wav' } : m))),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
  // The log line, in a contest and out of one.
  contestLogManual,
  contestLogManualRows: vi.fn(async () => []),
  contestWorking: vi.fn(async () => ({})),
  contestEntryReset: vi.fn(async () => ({})),
  contestPlace: vi.fn(async () => null),
  logQso: vi.fn(async () => ({})),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
  setLogFormGrid: vi.fn(async () => {}),
  // The Stats pane (shown here) asks about a call that stands in the log line; the question never answers.
  getCallStats: vi.fn(() => new Promise(() => {})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

afterEach(() => {
  cleanup()
  playVoiceMessage.mockClear()
  stopVoice.mockClear()
  startVoiceRecording.mockClear()
  contestLogManual.mockClear()
  localStorage.clear()
})

const ILQP = {
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
    { key: 'RST', raw: '59' },
    { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
  ],
  sentExchange: 'COOK',
} as unknown as FieldDayStatus

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
      nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

function fakePanels(): PanelLayoutApi<PhonePanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
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

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
/** The Phone cockpit as App gives it: in the ILQP contest with Enter Sends Message on, or out of a
 *  contest. */
async function renderCockpit({ contest = true } = {}) {
  render(
    <PhoneCockpit
      snap={makeSnap()}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={fakePanels()}
      fieldDay={contest ? ILQP : null}
      esmSetting={contest ? { on: true, callOnce: false, onSwitch: () => {} } : undefined}
    />,
  )
  await flush()
  await flush()
}
const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const casualCall = () => document.querySelector('.le-call') as HTMLInputElement
const qthBox = () => document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')[1]
/** Every box of the log line a key can be typed in. */
const logBoxes = () =>
  [...document.querySelectorAll<HTMLInputElement>('section.pane-frame[data-pane="log"] input')].filter(
    (i) => i.type === 'text' && !i.disabled,
  )
const line = () => document.querySelector('.le-fd-esm-line')?.textContent ?? null
const played = () => playVoiceMessage.mock.calls.map((c) => c[0])
const loggedCalls = () => contestLogManual.mock.calls.map((c) => c[0])

/** Press `key` with the cursor in `el`; true when nothing held it, so it goes on to the window. */
async function pressIn(el: Element, key: string, mods: KeyboardEventInit = {}) {
  if (el instanceof HTMLElement) el.focus()
  let free = false
  await act(async () => {
    free = fireEvent.keyDown(el, { key, ...mods })
  })
  await flush()
  return free
}
/** Type `text` into `box` as a browser does: each key goes down first, and its character goes in only
 *  when nothing held that key. */
async function typeInto(box: HTMLInputElement, text: string) {
  box.focus()
  for (const ch of text) {
    if (fireEvent.keyDown(box, { key: ch })) fireEvent.change(box, { target: { value: box.value + ch } })
  }
  await flush()
}
async function enter(el: Element) {
  fireEvent.keyDown(el, { key: 'Enter' })
  await flush()
  await flush()
}

const F1_TO_F12 = Array.from({ length: 12 }, (_, i) => `F${i + 1}`)
const MODES = [
  ['in a contest', true, stripCall],
  ['out of one', false, casualCall],
] as const

describe('the voice keyer’s F-keys from the log line', () => {
  it.each(MODES)('F1 in the Call box plays slot 1, %s', async (_mode, contest, call) => {
    await renderCockpit({ contest })
    expect(call(), 'the log line has no Call box').not.toBeNull()
    expect(await pressIn(call(), 'F1'), 'F1 reached the window').toBe(false)
    expect(played()).toEqual([1])
  })

  it.each(MODES)('F5 in the Call box is held, and so is every key F1–F12, %s', async (_mode, contest, call) => {
    await renderCockpit({ contest })
    expect(await pressIn(call(), 'F5'), 'F5 reached the window').toBe(false)
    for (const key of F1_TO_F12) {
      expect(await pressIn(call(), key), `${key} reached the window`).toBe(false)
    }
    // F5 and F6 hold no recording, and F7 and F9–F12 are no slot: only the recorded ones play.
    expect(played()).toEqual([1, 2, 3, 4, 8])
  })

  it.each(MODES)('a key pressed in any other box of the log line plays too, %s', async (_mode, contest) => {
    await renderCockpit({ contest })
    const boxes = logBoxes()
    expect(boxes.length, 'the log line has a box besides the Call box').toBeGreaterThan(1)
    for (const box of boxes) await pressIn(box, 'F2')
    expect(played()).toEqual(boxes.map(() => 2))
  })

  it.each(MODES)('a call typed in the Call box still types, %s', async (_mode, contest, call) => {
    await renderCockpit({ contest })
    await typeInto(call(), 'k9aaa')
    expect(call().value).toBe('K9AAA')
    expect(played()).toEqual([])
  })

  it('an F-key with Alt, Ctrl or Cmd is the system’s, in the Call box and out of it (Alt+F4 closes the window)', async () => {
    await renderCockpit()
    const chords: [string, KeyboardEventInit][] = [
      ['F4', { altKey: true }],
      ['F5', { ctrlKey: true }],
      ['F1', { metaKey: true }],
    ]
    for (const [key, mods] of chords) {
      const chord = `${Object.keys(mods)[0]}+${key}`
      expect(await pressIn(stripCall(), key, mods), `${chord} in the Call box was held`).toBe(true)
      expect(await pressIn(document.body, key, mods), `${chord} outside it was held`).toBe(true)
    }
    expect(played()).toEqual([])
  })

  it('nothing plays while a recording is in progress, from the Call box or outside it, and Enter Sends Message does not count the key', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    const f6 = [...document.querySelectorAll('.vk-slot')].find((s) => s.querySelector('.vk-fkey')?.textContent === 'F6')
    fireEvent.click(f6!.querySelector('.vk-tool')!) // ● on F6
    await flush()
    expect(startVoiceRecording, 'the keyer is not recording').toHaveBeenCalledTimes(1)
    for (const key of ['F1', 'F2', 'F8']) {
      await pressIn(stripCall(), key)
      await pressIn(document.body, key)
    }
    expect(played()).toEqual([])
    // F1, the CQ key, sent nothing, so ESM still searches and pounces: once the take is saved, Enter
    // on his call plays my call, F4, rather than leaving his call and the exchange to be said.
    fireEvent.click(screen.getByRole('button', { name: '■ Stop & save' }))
    await flush()
    await enter(stripCall())
    expect(played(), 'ESM ran on a CQ key that sent nothing').toEqual([4])
  })

  it('F1–F12 stay held while a recording is in progress, so F5 cannot reload the app over it', async () => {
    await renderCockpit()
    const f6 = [...document.querySelectorAll('.vk-slot')].find((s) => s.querySelector('.vk-fkey')?.textContent === 'F6')
    fireEvent.click(f6!.querySelector('.vk-tool')!) // ● on F6
    await flush()
    expect(startVoiceRecording, 'the keyer is not recording').toHaveBeenCalledTimes(1)
    for (const key of F1_TO_F12) {
      expect(await pressIn(stripCall(), key), `${key} in the Call box reached the window`).toBe(false)
      expect(await pressIn(document.body, key), `${key} outside it reached the window`).toBe(false)
    }
  })

  it('Esc stops: outside a box it is the keyer’s ■ Stop, and in one it is left to the shared Esc stop, as before', async () => {
    await renderCockpit()
    await pressIn(document.body, 'F1')
    expect(played()).toEqual([1])
    expect(await pressIn(document.body, 'Escape'), 'Esc reached the window').toBe(false)
    expect(stopVoice).toHaveBeenCalledTimes(1)
    stopVoice.mockClear()
    // In a box the over is stopped by the shared capture listener (App's halt, and ESM's stop),
    // which App.phoneFkeys.test.tsx drives; the keyer leaves the key alone there.
    expect(await pressIn(stripCall(), 'Escape'), 'the keyer held Esc in the Call box').toBe(true)
    expect(stopVoice).not.toHaveBeenCalled()
  })

  it('Enter Sends Message steps on F1 pressed in the Call box exactly as on F1 pressed outside it', async () => {
    /** A Run contact: CQ by F1 pressed in `where`, then Enter on his call and Enter on his QTH. */
    const contact = async (where: () => Element) => {
      await renderCockpit()
      const steps: { played: number[]; line: string | null; logged: unknown[] }[] = []
      const note = () => steps.push({ played: played(), line: line(), logged: loggedCalls() })
      await pressIn(where(), 'F1')
      note()
      fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
      await flush()
      await enter(stripCall())
      note()
      fireEvent.change(qthBox(), { target: { value: 'COOK' } })
      await flush()
      await enter(qthBox())
      note()
      cleanup()
      playVoiceMessage.mockClear()
      contestLogManual.mockClear()
      return steps
    }
    const outside = await contact(() => document.body)
    // Run, as PhoneCockpit.esm.test.tsx has it: CQ by hand, his call and the exchange said, TU logs.
    expect(outside[0].played).toEqual([1])
    expect(outside[1]).toMatchObject({ played: [1], line: 'Say his call and your exchange.', logged: [] })
    expect(outside[2]).toMatchObject({ played: [1, 3], logged: ['K9AAA'] })
    expect(await contact(stripCall)).toEqual(outside)
  })
})
