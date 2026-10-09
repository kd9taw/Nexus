// @vitest-environment jsdom
//
// ⭐ ENTER SENDS MESSAGE IN THE CW COCKPIT — the cockpit with its REAL contest strip, and an engine
// stand-in that does what the two send commands do: `send_cw` turns TX on and queues, and
// `send_cw_armed` (Enter's) refuses while TX is off and never turns it on. What it records is the
// order of the calls and the text the keyer was handed, so these hold the adapter to the F-keys'
// own path minus the re-arm: His Call becomes the strip's call before the send that expands `!`,
// and nothing logs until the engine took the message.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { EN } from '../i18n'
import type { AppSnapshot, FieldDayStatus } from '../types'
import type { LogQuestion } from '../features/logAnswers'

vi.setConfig({ testTimeout: 15_000 })

const engine = {
  peer: null as string | null,
  txEnabled: true,
  calls: [] as string[],
  keyed: [] as string[],
}

/** `expand_cw` for these tokens, in the Illinois QSO Party from Cook County. */
const expand = (text: string) =>
  [
    ['{MYCALL}', 'KD9TAW'],
    ['{RST}', '5NN'],
    ['{EXCH}', 'COOK'],
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

/** An operator's own CW profile, mapped N1MM-style: his call on F5, the exchange alone on F2,
 *  so "his call and my exchange" is F5 then F2, sent as one message. */
const MINE = {
  name: 'Mine',
  macros: [
    { key: 'F1', label: 'CQ', text: 'CQ IL {MYCALL}' },
    { key: 'F2', label: 'Exch', text: '5NN {EXCH}' },
    { key: 'F3', label: 'TU', text: 'TU {MYCALL}' },
    { key: 'F4', label: 'Me', text: '{MYCALL}' },
    { key: 'F5', label: 'Him', text: '!' },
    { key: 'F6', label: 'SP', text: 'TU 5NN {EXCH}' },
    { key: 'F7', label: 'AGN', text: 'AGN AGN' },
    { key: 'F8', label: 'B4', text: '! QSO B4' },
  ],
  esmRoles: { cq: ['F1'], callExch: ['F5', 'F2'], tu: ['F3'], myCall: ['F4'], exch: ['F6'], again: ['F7'] },
}
const settings = { macros: { cwProfiles: [MINE] as unknown[], activeCwProfile: 0 }, rigModel: 0 }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => settings),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({ ...decodeState, workedCall: engine.peer })),
    selectPeer: vi.fn(async (p: string | null) => {
      engine.calls.push(`selectPeer ${p}`)
      engine.peer = p
      return null
    }),
    // An F-key: turns TX on, by design, and queues.
    sendCw: vi.fn(async (text: string) => {
      engine.calls.push('sendCw')
      engine.txEnabled = true
      engine.keyed.push(expand(text))
      return {}
    }),
    // Enter: refuses while TX is off, and never turns it on.
    sendCwArmed: vi.fn(async (text: string) => {
      engine.calls.push('sendCwArmed')
      if (!engine.txEnabled) throw 'Not sent: TX is off, and Enter never turns it on.'
      engine.keyed.push(expand(text))
      return {}
    }),
    stopCw: vi.fn(async () => {
      engine.calls.push('stopCw')
      return {}
    }),
    haltTx: vi.fn(async () => {
      engine.calls.push('haltTx')
      engine.txEnabled = false
      return {}
    }),
    previewCw: vi.fn(async (t: string) => t),
    setCwWpm: vi.fn(async () => {}),
    setCwKeyer: vi.fn(async () => null),
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, [])),
    contestLogManual: vi.fn(async () => {
      engine.calls.push('contestLogManual')
      return true
    }),
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
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

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

function snap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', filterWidthHz: 500,
      splitTxMhz: null, smeterDb: null, ...over,
    },
  } as unknown as AppSnapshot
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

const onSwitch = vi.fn()
const cockpit = (s: AppSnapshot = snap(), on = true) => (
  <CwCockpit
    snap={s}
    theme="dark"
    onWorkSpot={() => {}}
    spots={[]}
    fieldDay={ILQP}
    esmSetting={{ on, callOnce: false, onSwitch }}
  />
)
async function renderCockpit(s?: AppSnapshot, on?: boolean) {
  const r = render(cockpit(s, on))
  await flush()
  await flush()
  return r
}

const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const qthBox = () => {
  const cap = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].find((n) => n.textContent === 'QTH')
  return cap!.closest('label')!.querySelector('input') as HTMLInputElement
}
const line = () => document.querySelector('.le-fd-esm-line')?.textContent ?? null
const lit = () =>
  [...document.querySelectorAll('.cw-macro.esm-lit .cw-macro-key')].map((n) => n.textContent)
const plate = () => document.querySelector('.esm-plate')
async function enter(el: Element) {
  fireEvent.keyDown(el, { key: 'Enter' })
  await flush()
  await flush()
}
/** Run: the key the set's CQ is on, pressed, which also calls CQ. */
async function run() {
  fireEvent.keyDown(window, { key: 'F1' })
  await flush()
}

beforeEach(() => {
  settings.macros.cwProfiles = [MINE]
  engine.peer = null
  engine.txEnabled = true
  engine.calls = []
  engine.keyed = []
  onSwitch.mockClear()
  for (const f of Object.values(api)) if (typeof f?.mockClear === 'function') f.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('Enter Sends Message in the CW cockpit', () => {
  it('runs the Illinois QSO Party on the built-in contest set: CQ TEST, his call and the exchange, TU and the log', async () => {
    settings.macros.cwProfiles = [] // no profile: the built-in set, N1MM's layout
    await renderCockpit()
    await run()
    expect(engine.keyed).toEqual(['CQ TEST DE KD9TAW KD9TAW K'])
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    expect(lit()).toEqual(['F2'])
    await enter(stripCall())
    expect(engine.keyed[engine.keyed.length - 1]).toBe('K9AAA 5NN COOK')
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    expect(lit()).toEqual(['F3'])
    engine.calls = []
    await enter(qthBox())
    expect(engine.keyed[engine.keyed.length - 1]).toBe('TU KD9TAW')
    expect(engine.calls.filter((c) => !c.startsWith('selectPeer'))).toEqual(['sendCwArmed', 'contestLogManual'])
  })

  it('searches and pounces on the built-in contest set: my call, then the S&P exchange and the log', async () => {
    settings.macros.cwProfiles = []
    await renderCockpit()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    expect(lit()).toEqual(['F4'])
    await enter(stripCall())
    expect(engine.keyed).toEqual(['KD9TAW'])
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    expect(lit()).toEqual(['F6'])
    await enter(qthBox())
    expect(engine.keyed).toEqual(['KD9TAW', 'TU 5NN COOK'])
    expect(engine.calls).toContain('contestLogManual')
  })

  it('sends through Enter’s own entry, never the F-keys’ re-arming one, with the strip’s call committed first', async () => {
    engine.peer = 'W1AW' // the station before
    await renderCockpit()
    await run()
    expect(engine.keyed, 'F1 called CQ').toEqual(['CQ IL KD9TAW'])
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    expect(lit(), 'the dock lights the two keys the next Enter sends').toEqual(['F2', 'F5'])
    engine.calls = []
    await enter(stripCall())
    expect(engine.calls, 'His Call is the strip’s call before the send expands `!`').toEqual([
      'selectPeer K9AAA',
      'sendCwArmed',
    ])
    expect(engine.keyed[engine.keyed.length - 1], 'F5 then F2, one message, to the strip’s call').toBe('K9AAA 5NN COOK')
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    expect(lit()).toEqual(['F3'])
    engine.calls = []
    await enter(qthBox())
    expect(
      engine.calls.filter((c) => !c.startsWith('selectPeer')),
      'TU goes, THEN the contact logs',
    ).toEqual(['sendCwArmed', 'contestLogManual'])
    expect(engine.keyed[engine.keyed.length - 1]).toBe('TU KD9TAW')
    expect(api.sendCw, 'Enter never used the send that turns TX on').toHaveBeenCalledTimes(1)
  })

  it('waits for His Call to be committed before the send that expands `!` to it', async () => {
    settings.macros.cwProfiles = []
    engine.peer = 'W1AW'
    await renderCockpit()
    await run()
    let release = () => {}
    vi.mocked(api.selectPeer).mockImplementationOnce(
      (p: string | null) =>
        new Promise((resolve) => {
          release = () => {
            engine.peer = p
            resolve(null)
          }
        }),
    )
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    engine.keyed = []
    await enter(stripCall())
    expect(engine.keyed, 'nothing keys while His Call is still being committed').toEqual([])
    await act(async () => release())
    await flush()
    expect(engine.keyed).toEqual(['K9AAA 5NN COOK'])
  })

  it('with TX off, Enter sends nothing, logs nothing, never turns TX on, and says why', async () => {
    engine.txEnabled = false
    await renderCockpit(snap({ txEnabled: false }))
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(engine.calls.filter((c) => c !== 'selectPeer null')).toEqual([])
    expect(engine.txEnabled).toBe(false)
    expect(line()).toBe(EN['contest.esm.refused.txOff'])
  })

  it('when the engine refuses after all, nothing logs and the strip says the engine’s reason', async () => {
    await renderCockpit() // the cockpit still reads TX on…
    engine.txEnabled = false // …and the engine has just halted
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(engine.calls).toContain('sendCwArmed')
    expect(engine.calls).not.toContain('contestLogManual')
    expect(engine.keyed).toEqual([])
    expect(engine.txEnabled).toBe(false)
    expect(line()).toBe('Nothing was sent, and nothing was logged: Not sent: TX is off, and Enter never turns it on.')
  })

  it('Stop TX stops, and what went out counts as not sent: the next Enter sends his call and the exchange again', async () => {
    await renderCockpit()
    await run()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    await flush()
    await enter(stripCall())
    fireEvent.click(screen.getByRole('button', { name: /stop tx/i }))
    await flush()
    expect(engine.calls.slice(-2), 'the stop is the stop it always was').toEqual(['stopCw', 'haltTx'])
    expect(line()).toBe('His call and your exchange stopped, so it counts as not sent: the next Enter sends it again.')
    engine.txEnabled = true // the operator turns TX back on with an F-key
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    engine.keyed = []
    await enter(qthBox())
    expect(engine.keyed).toEqual(['K9AAA 5NN COOK'])
    expect(engine.calls).not.toContain('contestLogManual')
  })

  it('the dock’s ESM switch saves through its own handler, and shows Run or S&P', async () => {
    await renderCockpit()
    const sw = screen.getByRole('switch', { name: EN['contest.esm.switch.label'] })
    expect(sw.getAttribute('aria-checked')).toBe('true')
    expect(plate()?.querySelector('.esm-mode')?.textContent).toBe('S&P')
    await run()
    expect(plate()?.querySelector('.esm-mode')?.textContent).toBe('Run')
    fireEvent.click(sw)
    expect(onSwitch).toHaveBeenCalledWith(false)
  })

  it('switched off, Enter only logs, and the plate shows only the switch', async () => {
    await renderCockpit(snap(), false)
    expect(plate()?.querySelector('.esm-mode')).toBeNull()
    fireEvent.change(stripCall(), { target: { value: 'K9AAA' } })
    fireEvent.change(qthBox(), { target: { value: 'COOK' } })
    await flush()
    await enter(qthBox())
    expect(engine.calls).toContain('contestLogManual')
    expect(api.sendCwArmed).not.toHaveBeenCalled()
  })
})
