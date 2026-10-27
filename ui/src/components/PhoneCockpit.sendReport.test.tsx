// @vitest-environment jsdom
//
// THE PHONE COCKPIT SAYS THE REPORT A PHONE CONTACT SENDS. A contest session holds its signal
// report as the 3-digit constant (599), because one session runs across CW and phone; a contact
// logged on phone is stamped 59 (`ContestSession::fit_sent_rst`). What the cockpit shows the operator
// to READ ALOUD — the Give line beside PTT, and the strip's Sending line — is the phone form, so what
// is said is what is logged. Found in an Illinois QSO Party rehearsal: Phone showed "599 MCHE" while
// the contact went into the Cabrillo file as 59.
//
// The REAL cockpit and contest strip, the harness of PhoneCockpit.esm.test.tsx.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
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
  localStorage.clear()
})

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const ilqp = (phoneForm: boolean) =>
  ({
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
      { key: 'QTH', raw: 'MCHE', domain: 'il_counties' },
    ],
    composingText: '599 MCHE',
    ...(phoneForm ? { composingTextPhone: '59 MCHE' } : {}),
    sentExchange: 'MCHE',
  }) as unknown as FieldDayStatus

const snap = {
  mycall: 'KD9TAW',
  hunt: null,
  radio: {
    dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
    transmitting: false, txEnabled: false, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
    nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: null,
    splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null,
  },
} as unknown as AppSnapshot

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

async function renderCockpit(fieldDay: FieldDayStatus) {
  render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={fakePanels()} fieldDay={fieldDay} />)
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
const give = () => document.querySelector('.ph-fd-give-exch')?.textContent
const sending = () => document.querySelector('.le-fd-sent-val')?.textContent

describe('the Phone cockpit says the report a phone contact sends', () => {
  it('the Give line and the Sending line read 59, not the session’s 599', async () => {
    await renderCockpit(ilqp(true))
    expect(give(), 'Give').toBe('59 MCHE')
    expect(sending(), 'Sending').toBe('59 MCHE')
  })

  it('CONTROL: a station that sends no phone form (an older one, or the Remote page) shows what it always did', async () => {
    await renderCockpit(ilqp(false))
    expect(give(), 'Give').toBe('599 MCHE')
    expect(sending(), 'Sending').toBe('599 MCHE')
  })
})
