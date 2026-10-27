// @vitest-environment jsdom
//
// ⭐ ENTER SENDS MESSAGE IN THE PHONE COCKPIT — the REAL cockpit, voice keyer and contest strip.
// Enter plays a recording through the keyer's own checks (empty slot, recording, PTT held, TX off)
// and the engine's; when running, his call and the exchange play nothing, because the operator
// says them (decision 10); and with the keyer hidden ESM steps aside, and Enter logs as it does
// with ESM off. The radio holding the mic (Nexus's own Flex client, native audio on) is no
// refusal: the station switches the radio to DAX around each message.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { pushToast } from '../toast'
import type { AppSnapshot, FieldDayStatus } from '../types'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

vi.setConfig({ testTimeout: 30_000 })

const { playVoiceMessage, stopVoice, contestLogManual } = vi.hoisted(() => ({
  playVoiceMessage: vi.fn(async (_slot: number) => ({})),
  stopVoice: vi.fn(async () => ({})),
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
  // The voice keyer as the update leaves a station that ran ESM on the built-in steps
  // (`Settings::move_voice_agn_to_f7`): CQ, my exchange, TU and my call on F1–F4 as they were,
  // under the slots' default labels; the AGN recording copied from F5 to F8's own file; F5 and
  // F6 empty.
  getVoiceMessages: vi.fn(async () => [
    { slot: 1, label: 'CQ', file: '/voice/slot1.wav' },
    { slot: 2, label: 'My Call', file: '/voice/slot2.wav' },
    { slot: 3, label: 'Report', file: '/voice/slot3.wav' },
    { slot: 4, label: 'QRZ?', file: '/voice/slot4.wav' },
    { slot: 5, label: '73', file: '' },
    { slot: 6, label: 'Again', file: '' },
    { slot: 8, label: 'AGN', file: '/voice/slot8.wav' },
  ]),
  playVoiceMessage,
  stopVoice,
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
  // The contest strip.
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
  contestLogManual.mockClear()
  vi.mocked(pushToast).mockClear()
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

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
      nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null, ...over,
    },
  } as unknown as AppSnapshot
}

function fakePanels(removed: PhonePanelId[] = []): PanelLayoutApi<PhonePanelId> {
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

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
async function renderCockpit({ removed = [] as PhonePanelId[], snap = makeSnap(), callOnce = false } = {}) {
  render(
    <PhoneCockpit
      snap={snap}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={fakePanels(removed)}
      fieldDay={ILQP}
      esmSetting={{ on: true, callOnce, onSwitch: () => {} }}
    />,
  )
  await flush()
  await flush()
}
const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const boxes = () => [...document.querySelectorAll<HTMLInputElement>('.le-fd-input-code')]
const qthBox = () => boxes()[1]
const line = () => document.querySelector('.le-fd-esm-line')?.textContent ?? null
async function enter(el: Element) {
  fireEvent.keyDown(el, { key: 'Enter' })
  await flush()
  await flush()
}
const played = () => playVoiceMessage.mock.calls.map((c) => c[0])
const loggedCalls = () => contestLogManual.mock.calls.map((c) => c[0])

describe('Enter Sends Message in the Phone cockpit', () => {
  it('searches and pounces: plays my call, then my exchange, which logs the contact', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    expect(played(), 'F4, my call').toEqual([4])
    expect(loggedCalls()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played(), 'then F2, my exchange').toEqual([4, 2])
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('runs: CQ by hand switches to Run; his call and the exchange play nothing; TU plays and logs', async () => {
    await renderCockpit()
    fireEvent.keyDown(document.body, { key: 'F1' }) // the keyer's F1, by hand: CQ
    await flush()
    expect(played()).toEqual([1])
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    expect(played(), 'nothing plays: the operator says it').toEqual([1])
    expect(line()).toBe('Say his call and your exchange.')
    expect(document.activeElement, 'the caret is in his first box').toBe(boxes()[0])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played(), 'F3, TU').toEqual([1, 3])
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  // ⭐ AGN IS ON F8, as the CW and RTTY contest sets send it (the
  // operator, 2026-10-10); it was F5. Every other step plays the slot it played before (CQ F1, my
  // exchange F2, TU F3, my call F4), and AGN plays F8, which holds the recording F5 held: the
  // settings tests prove that copy byte for byte (`agn_f8_tests`). So each Enter sends the message
  // it sent before.
  it('runs: Enter with his exchange not yet copied asks AGN, from F8', async () => {
    await renderCockpit()
    fireEvent.keyDown(document.body, { key: 'F1' }) // the keyer's F1, by hand: CQ, and Run
    await flush()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall()) // his call and the exchange: said, not played
    await enter(qthBox()) // his QTH not copied yet
    expect(played(), 'CQ, then AGN from F8').toEqual([1, 8])
    expect(loggedCalls()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played(), 'then TU, which logs').toEqual([1, 8, 3])
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  // ⭐ A DUPE CALLS WHILE YOU RUN (the operator, 2026-10-10: one rule). The keyer has no
  // QSO B4 slot — its slots are F1–F6 and F8, none labelled B4 — so Enter plays nothing for a dupe
  // and the strip names the step, until a slot you recorded is mapped to it in Settings.
  describe('a dupe while running', () => {
    const worked = {
      ...ILQP,
      log: [{ call: 'K9AAA', class: '', section: '', band: '20m', mode: 'PH', submode: 'USB', whenUnix: 1_792_342_800, rcvd: ['59', 'COOK'] }],
    } as unknown as FieldDayStatus
    const mount = async (voiceRoles?: Record<string, string[]>) => {
      render(
        <PhoneCockpit
          snap={makeSnap()}
          theme="dark"
          onWorkSpot={() => {}}
          spots={[]}
          panels={fakePanels()}
          fieldDay={worked}
          esmSetting={{ on: true, callOnce: false, onSwitch: () => {}, voiceRoles }}
        />,
      )
      await flush()
      await flush()
      fireEvent.keyDown(document.body, { key: 'F1' }) // the keyer's F1, by hand: CQ, and Run
      await flush()
      fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
      fireEvent.change(qthBox(), { target: { value: 'COOK' } })
      await flush()
    }

    it('plays nothing on the built-in steps, where no slot holds QSO B4, and the strip names the step', async () => {
      await mount()
      await enter(qthBox())
      expect(played(), 'CQ only').toEqual([1])
      expect(loggedCalls()).toEqual([])
      expect(line()).toBe('QSO B4: no key is mapped, so Enter sends nothing and logs nothing at that step.')
    })

    it('names the slot to record when QSO B4 is mapped to an empty one', async () => {
      await mount({ b4: ['F6'] })
      await enter(qthBox())
      expect(played()).toEqual([1])
      expect(line()).toBe('QSO B4: F6 is empty, so Enter sends nothing and logs nothing at that step.')
    })

    it('plays the recorded slot mapped to QSO B4 once, and the next Enter logs the dupe, playing nothing', async () => {
      const api = await import('../api')
      const slots = vi.mocked(api.getVoiceMessages)
      const shipped = slots.getMockImplementation()!
      slots.mockImplementation(async () =>
        (await shipped()).map((m) => (m.slot === 6 ? { ...m, label: 'QSO B4', file: '/voice/slot6.wav' } : m)),
      )
      try {
        await mount({ b4: ['F6'] })
        await enter(qthBox())
        expect(played(), 'CQ, then F6').toEqual([1, 6])
        expect(loggedCalls()).toEqual([])
        await enter(qthBox())
        expect(played(), 'nothing more plays').toEqual([1, 6])
        expect(loggedCalls()).toEqual(['K9AAA'])
      } finally {
        slots.mockImplementation(shipped)
      }
    })
  })

  it('searches and pounces with call once: the next Enter asks AGN, from F8', async () => {
    await renderCockpit({ callOnce: true })
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall()) // my call, once
    await enter(qthBox()) // his QTH not copied yet
    expect(played(), 'my call, then AGN from F8').toEqual([4, 8])
    expect(loggedCalls()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played(), 'then my exchange, which logs').toEqual([4, 8, 2])
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('steps aside while the keyer is hidden: nothing plays, Enter logs, and the plate says why', async () => {
    await renderCockpit({ removed: ['voiceKeyer'] })
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played()).toEqual([])
    expect(loggedCalls()).toEqual(['K9AAA'])
    expect(document.querySelector('.esm-next')?.textContent).toBe(
      'ESM steps aside while the voice keyer is hidden: Enter logs as it does with ESM off.',
    )
  })

  // The radio having the mic (Nexus's own Flex client, native audio on) is no refusal since the
  // operator's ruling of 2026-10-12, "Change it now": the station switches the radio to DAX for
  // each message.
  it('plays while the radio has the mic: my call, then my exchange, which logs the contact', async () => {
    await renderCockpit({ snap: makeSnap({ flexRadioHasMic: true }) })
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    expect(played(), 'F4, my call').toEqual([4])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played(), 'then F2, my exchange').toEqual([4, 2])
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('refuses while PTT is held: nothing plays and nothing logs', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    fireEvent.pointerDown(screen.getByRole('button', { name: /push to talk/i }))
    await flush()
    await enter(qthBox())
    expect(played()).toEqual([])
    expect(loggedCalls()).toEqual([])
    expect(line()).toBe('You are holding PTT, so Enter plays nothing.')
  })

  it('the keyer’s ■ Stop is a stop: what it played counts as not sent', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall()) // my call
    fireEvent.click(screen.getByRole('button', { name: '■ Stop' }))
    await flush()
    expect(stopVoice).toHaveBeenCalled()
    expect(line()).toBe('Your call stopped, so it counts as not sent: the next Enter sends it again.')
  })

  it('Esc is a stop too: a stopped S&P exchange stays logged, and the strip says so', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(loggedCalls()).toEqual(['K9AAA'])
    fireEvent.keyDown(stripCall(), { key: 'Escape' })
    await flush()
    expect(line()).toBe('Your S&P exchange stopped. K9AAA is logged. Ctrl+D twice removes it.')
  })
})

// ⭐ THE SCORE CHIP rides in this cockpit's Log pane HEAD, beside the one-word title — the strip below
// sits in a 24–44em column whose own header row has no room left — and the strip carries no second copy.
describe('the contest score chip', () => {
  it('reads the running contest in the Log pane’s head', async () => {
    await renderCockpit()
    const head = document.querySelector('section.pane-frame[data-pane="log"] > .pane-head')
    expect(head?.querySelector('.contest-score-chip')?.textContent ?? null).toBe('ILQP · 0 Q · 0 sections · —/hr')
    expect(document.querySelector('.log-entry-fd')).not.toBeNull()
    expect(document.querySelector('.le-fd-score')).toBeNull()
  })
})

// ⭐ THE VOICE KEYER WHILE THE RADIO HAS THE MIC (Nexus's own Flex client, native DAX audio on, Phone at
// the shack): no refusal since the operator's ruling of 2026-10-12, "Change it now". The station
// switches the radio to DAX around each message, so a slot's click and its F-key play, with no toast.
describe('the voice keyer while the radio has the mic', () => {
  it('plays by click and by F-key, with no toast', async () => {
    await renderCockpit({ snap: makeSnap({ flexRadioHasMic: true }) })
    const f1 = document.querySelector<HTMLButtonElement>('.vk-play')
    expect(f1, 'precondition: F1 holds a message').not.toBeNull()
    fireEvent.click(f1!)
    await flush()
    fireEvent.keyDown(window, { key: 'F1' })
    await flush()
    expect(played()).toEqual([1, 1])
    expect(vi.mocked(pushToast)).not.toHaveBeenCalled()
  })
})
