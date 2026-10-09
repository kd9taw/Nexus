// @vitest-environment jsdom
//
// THE LOG STRIP'S CALL IS THE CALL THE F-KEYS SEND.
//
// `!` in a CW macro is the engine's ACTIVE PEER at the moment `send_cw` expands it
// (`Engine::expand_cw` → `app.active_peer()`), and `select_peer` is the only thing that sets
// it: His Call, a decoded chip, a spot handoff. The log strip bound its call to the contest
// session and never to the peer, so typing K9AAA in the strip and pressing F3 keyed whatever
// His Call held: the last station, or no call at all. The strip and His Call are now ONE
// FIELD shown twice, as RTTY's Call box and strip are: a call typed in either is in both,
// and it is committed to the peer before the next send.
//
// The cockpit renders with the REAL log strip. The engine is a stand-in that does the two
// things these keys reach: it holds the peer `selectPeer` sets, and it expands a macro the
// way `expand_cw` does for the Illinois QSO Party below (the real expansion, contest by
// contest, is `tempo-app/tests/cw_contest_macros.rs`). What it records is the text the
// keyer is handed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { EN } from '../i18n'
import type { AppSnapshot, FieldDayStatus } from '../types'
import type { LogQuestion } from '../features/logAnswers'

const engine = { peer: null as string | null, keyed: [] as string[] }

/** `expand_cw` for the tokens these macros carry, in the Illinois QSO Party from Cook County. */
const expand = (text: string) =>
  [
    ['{MYCALL}', 'KD9TAW'],
    ['{RST}', '5NN'],
    ['{EXCH}', 'COOK'],
    ['{NAME}', ''],
    ['!', engine.peer ?? ''],
  ]
    .reduce((out, [token, value]) => out.split(token).join(value), text)
    .split(/\s+/)
    .filter(Boolean)
    .join(' ')

const decodeState = {
  text: '',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

vi.mock('../api', async (importOriginal) => {
  // Every export auto-stubbed, so a call the strip makes that this list does not name cannot
  // throw on mount; the entries below are the ones these assertions depend on.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    // The engine: `cw_decode` reports the peer back as the worked call, as `read_cw_state` does.
    cwDecode: vi.fn(async () => ({ ...decodeState, workedCall: engine.peer })),
    selectPeer: vi.fn(async (p: string | null) => {
      engine.peer = p
      return null
    }),
    sendCw: vi.fn(async (text: string) => {
      engine.keyed.push(expand(text))
      return {}
    }),
    previewCw: vi.fn(async (t: string) => t),
    setCwWpm: vi.fn(async () => {}),
    setCwKeyer: vi.fn(async () => null),
    // The strip.
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
    contestLogManual: vi.fn(async () => true),
    contestLogManualRows: vi.fn(async (_call: string, rows: unknown[]) => rows.map(() => true)),
    contestWorking: vi.fn(async () => ({})),
    contestZoneHint: vi.fn(async () => null),
    logQso: vi.fn(async () => ({})),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    setCwPeerInfo: vi.fn(async () => {}),
    setLogFormGrid: vi.fn(async () => {}),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** The Illinois QSO Party from Cook County, as the engine serialises it. */
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

function snap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500,
      splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}
/** Past the settle that carries a call typed into His Call into the strip (500 ms). */
async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 600))
  })
  await flush()
}

async function renderCockpit(fieldDay: FieldDayStatus | null = ILQP) {
  const r = render(<CwCockpit snap={snap()} theme="dark" onWorkSpot={() => {}} spots={[]} fieldDay={fieldDay} />)
  // The mount-time settings read and the first decode poll, which reports the engine's peer.
  await flush()
  await flush()
  return r
}

const hisCall = () => screen.getByRole('textbox', { name: EN['cw.hisCall.label'] }) as HTMLInputElement
const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const casualCall = () => screen.getByPlaceholderText(EN['logEntry.call.placeholder']) as HTMLInputElement
const qthBox = () => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent === 'QTH')
  return cap!.closest('label')!.querySelector('input') as HTMLInputElement
}
async function press(key: string, target: Element | Window = window) {
  fireEvent.keyDown(target, { key })
  await flush()
  await flush()
}

beforeEach(() => {
  engine.peer = null
  engine.keyed = []
  decodeState.candidates = []
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the CW log strip and His Call are one field', () => {
  it('F3 after a call typed only in the strip keys that call, not the last station', async () => {
    engine.peer = 'W1AW' // the station before
    await renderCockpit()
    expect(hisCall().value).toBe('W1AW')
    fireEvent.change(stripCall(), { target: { value: 'k9aaa' } })
    await press('F3', stripCall())
    expect(engine.keyed).toEqual(['K9AAA DE KD9TAW 5NN COOK COOK K'])
    expect(hisCall().value, 'His Call shows the strip').toBe('K9AAA')
  })

  it('…and with no station before, the call is still the one typed', async () => {
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await press('F3', stripCall())
    expect(engine.keyed).toEqual(['K9AAA DE KD9TAW 5NN COOK COOK K'])
  })

  it('outside a contest too: the log strip\'s call is the one F2 keys', async () => {
    await renderCockpit(null)
    fireEvent.change(casualCall(), { target: { value: 'K9AAA' } })
    await press('F2', casualCall())
    expect(engine.keyed).toEqual(['K9AAA DE KD9TAW KD9TAW K'])
  })

  it('a call typed into His Call shows in the strip', async () => {
    engine.peer = 'W1AW'
    await renderCockpit()
    fireEvent.change(hisCall(), { target: { value: 'N0CALL' } })
    await settle()
    expect(stripCall().value).toBe('N0CALL')
  })

  it('logging the contact empties both, so the next F3 keys no call', async () => {
    engine.peer = 'W1AW'
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'KANE' } })
    fireEvent.keyDown(qthBox(), { key: 'Enter' })
    await flush()
    await flush()
    expect(stripCall().value).toBe('')
    expect(hisCall().value).toBe('')
    await press('F3')
    expect(engine.keyed).toEqual(['DE KD9TAW 5NN COOK COOK K'])
  })

  // ── CONTROLS: what must not move ──────────────────────────────────────────────────────

  it('His Call typed directly is what F3 keys', async () => {
    engine.peer = 'W1AW'
    await renderCockpit()
    fireEvent.change(hisCall(), { target: { value: 'N0CALL' } })
    await press('F3', hisCall())
    expect(engine.keyed).toEqual(['N0CALL DE KD9TAW 5NN COOK COOK K'])
  })

  it('a decoded-chip pick is what F3 keys', async () => {
    decodeState.candidates = [{ call: 'W2BBB', best: true }]
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    fireEvent.click(screen.getByRole('button', { name: 'W2BBB' }))
    await settle()
    await press('F3')
    expect(engine.keyed).toEqual(['W2BBB DE KD9TAW 5NN COOK COOK K'])
  })

  it('…and it fills both fields, over a call typed in the strip', async () => {
    decodeState.candidates = [{ call: 'W2BBB', best: true }]
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    fireEvent.click(screen.getByRole('button', { name: 'W2BBB' }))
    await settle()
    expect(hisCall().value).toBe('W2BBB')
    expect(stripCall().value).toBe('W2BBB')
  })

  it('a decoder\'s best guess fills neither field until it is picked', async () => {
    decodeState.candidates = [{ call: 'W2BBB', best: true }]
    await renderCockpit()
    await settle()
    expect(stripCall().value).toBe('')
    expect(hisCall().value).toBe('')
    await press('F3')
    expect(engine.keyed).toEqual(['DE KD9TAW 5NN COOK COOK K'])
  })
})
