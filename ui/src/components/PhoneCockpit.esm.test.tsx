// @vitest-environment jsdom
//
// ⭐ ENTER SENDS MESSAGE IN THE PHONE COCKPIT — the REAL cockpit, voice keyer and contest strip.
// Enter plays a recording through the keyer's own checks (empty slot, recording, PTT held, the
// radio holding the mic, TX off) and the engine's; when running, his call and the exchange play
// nothing, because the operator says them (decision 10); and with the keyer hidden ESM steps
// aside, and Enter logs as it does with ESM off.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
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
  // The voice keyer: the five ESM slots recorded, F6 empty.
  getVoiceMessages: vi.fn(async () =>
    ['CQ', 'Exch', 'TU', 'My call', 'AGN'].map((label, i) => ({ slot: i + 1, label, file: `/tmp/f${i + 1}.wav` })).concat([
      { slot: 6, label: '', file: '' },
    ]),
  ),
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
  contestZoneHint: vi.fn(async () => null),
  logQso: vi.fn(async () => ({})),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
  setLogFormGrid: vi.fn(async () => {}),
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
async function renderCockpit({ removed = [] as PhonePanelId[], snap = makeSnap() } = {}) {
  render(
    <PhoneCockpit
      snap={snap}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      panels={fakePanels(removed)}
      fieldDay={ILQP}
      esmSetting={{ on: true, callOnce: false, onSwitch: () => {} }}
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

  it('refuses while the radio has the mic: nothing plays, nothing logs, and the contact stays in the strip', async () => {
    await renderCockpit({ snap: makeSnap({ flexRadioHasMic: true }) })
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(played()).toEqual([])
    expect(loggedCalls()).toEqual([])
    expect(line()).toBe('The radio has the mic, so a recording would not go out.')
    expect(stripCall().value).toBe('K9AAA')
    expect(qthBox().value).toBe('COOK')
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
