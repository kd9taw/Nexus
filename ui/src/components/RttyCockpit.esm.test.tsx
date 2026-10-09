// @vitest-environment jsdom
//
// ⭐ ENTER SENDS MESSAGE IN THE RTTY COCKPIT — the cockpit with its REAL contest strip. Enter sends
// through THE ONE PATH to `rtty_send` the F-keys use (`prepare`, then `rttySend`), framed for the
// air and expanded for the call in the strip, and steps aside while the auto sequence runs or
// Continuous TX is latched (decision 9): Enter then logs exactly as it does with ESM off.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, FieldDayStatus, RttyState, Settings } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const state: { current: RttyState } = { current: {} as RttyState }
const idle = (): RttyState =>
  ({
    armed: true,
    afcHz: 0,
    afcLocked: false,
    text: '',
    charConf: [],
    baud: 45.45,
    shiftHz: 170,
    markHz: 2125,
    spaceHz: 2295,
    sending: false,
    latched: false,
    backend: 'afsk',
    keyerError: null,
    auto: false,
    seqState: 'idle',
    peer: null,
    peerExchange: [],
    heardCq: null,
  }) as unknown as RttyState

const sent: string[] = []
const contestLogManual = vi.fn(async (..._args: unknown[]) => ({}))

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(async () => state.current),
  rttyAutoArm: vi.fn(async () => state.current),
  rttySend: vi.fn(async (text: string) => {
    sent.push(text)
    return state.current
  }),
  rttyStop: vi.fn(async () => state.current),
  rttyClear: vi.fn(async () => state.current),
  rttyAfcReset: vi.fn(async () => state.current),
  rttyNet: vi.fn(async () => state.current),
  rttySetAuto: vi.fn(async () => state.current),
  rttySetLatched: vi.fn(async () => state.current),
  rttyType: vi.fn(async () => state.current),
  rttyAutoCq: vi.fn(async () => state.current),
  rttyAutoAnswer: vi.fn(async () => state.current),
  rttyAutoAbort: vi.fn(async () => state.current),
  setRfPower: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  atuTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setRttyMacros: vi.fn(async () => ({})),
  logQso: vi.fn(async () => ({})),
  contestLogManual: (...args: unknown[]) => contestLogManual(...args),
  contestWorking: async () => ({}),
  contestEntryReset: async () => ({}),
  contestZoneHint: async () => null,
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
  setLogFormGrid: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

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
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: 'COOK', domain: 'il_counties' },
  ],
  sentExchange: 'COOK',
} as unknown as FieldDayStatus

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  hunt: null,
  b4MatchMode: false,
  radio: { dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, txAllowed: true },
  fieldDay: ILQP,
} as unknown as AppSnapshot

/** RTTY's Contest set, as it ships: N1MM's layout. */
const CONTEST_SET = { rttyProfiles: [], activeRttyProfile: 'contest' } as unknown as Settings['macros']

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
async function renderCockpit() {
  const r = render(
    <RttyCockpit snap={snap} macros={CONTEST_SET} esmSetting={{ on: true, callOnce: false, onSwitch: () => {} }} />,
  )
  await flush()
  await flush()
  return r
}
const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const qthBox = () => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent === 'QTH')
  return cap!.closest('label')!.querySelector('input') as HTMLInputElement
}
async function enter(el: Element) {
  fireEvent.keyDown(el, { key: 'Enter' })
  await flush()
  await flush()
}
const loggedCalls = () => contestLogManual.mock.calls.map((c) => c[0])

beforeEach(() => {
  state.current = idle()
  sent.length = 0
  contestLogManual.mockClear()
  for (const f of Object.values(api)) if (typeof f?.mockClear === 'function') f.mockClear()
})
afterEach(cleanup)

describe('Enter Sends Message in the RTTY cockpit', () => {
  it('runs a contact through the one path to rtty_send: CQ (F1), his call and the exchange, then TU, which logs it', async () => {
    await renderCockpit()
    fireEvent.keyDown(window, { key: 'F1' }) // CQ, by hand: ESM goes to Run
    await flush()
    expect(sent).toEqual(['\r\nCQ TEST KD9TAW KD9TAW CQ '])
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    expect(sent[sent.length - 1], 'F2, framed for the air, to the call in the strip').toBe('\r\nK9AAA 599 COOK COOK ')
    expect(loggedCalls()).toEqual([])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(sent[sent.length - 1]).toBe('\r\nTU KD9TAW CQ ')
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('searches and pounces: my call, then the S&P exchange, which logs it', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    expect(sent).toEqual(['\r\nKD9TAW KD9TAW '])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(sent[sent.length - 1]).toBe('\r\nTU 599 COOK COOK ')
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('steps aside while the auto sequence runs: Enter logs and sends nothing', async () => {
    state.current = { ...idle(), auto: true, seqState: 'answering' } as RttyState
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(sent).toEqual([])
    expect(loggedCalls()).toEqual(['K9AAA'])
    expect(document.querySelector('.esm-next')?.textContent).toBe(
      'ESM steps aside while the RTTY auto sequence runs: Enter logs as it does with ESM off.',
    )
  })

  it('steps aside while Continuous TX is latched: Enter logs and types nothing into the stream', async () => {
    state.current = { ...idle(), latched: true } as RttyState
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(sent).toEqual([])
    expect(api.rttyType).not.toHaveBeenCalled()
    expect(loggedCalls()).toEqual(['K9AAA'])
  })

  it('Stop makes what went out count as not sent: the next Enter sends his call and the exchange again', async () => {
    await renderCockpit()
    fireEvent.keyDown(window, { key: 'F1' })
    await flush()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    fireEvent.keyDown(window, { key: 'Escape' })
    await flush()
    expect(api.rttyStop).toHaveBeenCalled()
    expect(api.haltTx).toHaveBeenCalled()
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    sent.length = 0
    await enter(qthBox())
    expect(sent).toEqual(['\r\nK9AAA 599 COOK COOK '])
    expect(loggedCalls()).toEqual([])
  })
})
