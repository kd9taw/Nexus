// @vitest-environment jsdom
//
// THE TX LATCH IN CW IS A BUTTON ON A COMPUTER DRIVING A STATION. At the station, entering CW arms
// TX and the latch beside Tune is read-only. From afar, entering a section arms nothing, so the
// latch read "TX Off" and did nothing when clicked, and Enter Sends Message (which never turns TX
// on) refused every press. There it is now the latch every other screen has: one click is one
// `set_tx_enabled`, through the station's own guards, and a refusal is shown. Once armed, Enter
// sends; Enter alone still never arms. CONTROL: at the station it stays read-only.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const { pushToast, driving } = vi.hoisted(() => ({
  pushToast: vi.fn(),
  driving: { value: true },
}))

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({
      text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '',
      prompt: '', recommended: null, workedCall: null, rst: null, name: null,
    })),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    setCwKeyer: vi.fn(async () => null),
    askLog: vi.fn(async (q: unknown) =>
      (await import('../features/logAnswers.testkit')).answerAs(q as never, []),
    ),
    contestLogManual: vi.fn(async () => true),
    contestLogManualRows: vi.fn(async (_call: string, rows: unknown[]) => rows.map(() => true)),
    contestPlace: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.value,
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

function makeSnap(txEnabled: boolean): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    hunt: null,
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled, txAllowed: true, tuning: false, atu: false, cwWpm: 22, cwKeyer: 'cat',
      filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

/** The cockpit as App hosts it: a returned snapshot is the next render's. */
async function renderCw(txEnabled = false) {
  let rerender: (ui: React.ReactElement) => void = () => {}
  const cockpit = (s: AppSnapshot) => (
    <CwCockpit
      snap={s}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      fieldDay={ILQP}
      esmSetting={{ on: true, callOnce: false, onSwitch: () => {} }}
      onSnap={(next) => rerender(cockpit(next))}
    />
  )
  ;({ rerender } = render(cockpit(makeSnap(txEnabled))))
  await flush()
  await flush()
}

const latchButton = () => screen.queryByRole('button', { name: /^tx on$|^tx off$/i })
const stripCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement

async function click(el: HTMLElement) {
  await act(async () => {
    fireEvent.click(el)
  })
  await flush()
}
async function enterOnCall(call: string) {
  fireEvent.change(stripCall(), { target: { value: call } })
  await flush()
  fireEvent.keyDown(stripCall(), { key: 'Enter' })
  await flush()
  await flush()
}

beforeEach(() => {
  driving.value = true
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  api.setTxEnabled.mockImplementation(async (on: boolean) => makeSnap(on))
})

afterEach(() => {
  cleanup()
  pushToast.mockClear()
  for (const f of Object.values(api)) if (typeof f?.mockClear === 'function') f.mockClear()
})

describe('the CW TX latch on a computer driving a station', () => {
  it('is a button, and one click arms TX once', async () => {
    await renderCw(false)
    const latch = latchButton()
    expect(latch, 'the latch beside Tune is a button from afar').not.toBeNull()
    await click(latch!)
    expect(api.setTxEnabled.mock.calls).toEqual([[true]])
    expect(api.setPtt, 'arming keys nothing').not.toHaveBeenCalled()
    expect(api.sendCw, 'arming sends nothing').not.toHaveBeenCalled()
    expect(api.sendCwArmed, 'arming sends nothing').not.toHaveBeenCalled()
  })

  it('one click disarms TX once', async () => {
    await renderCw(true)
    await click(latchButton()!)
    expect(api.setTxEnabled.mock.calls).toEqual([[false]])
    expect(api.setPtt).not.toHaveBeenCalled()
  })

  it('shows the station’s refusal', async () => {
    api.setTxEnabled.mockImplementation(async () => {
      throw 'Transmit is held until this device signs in.'
    })
    await renderCw(false)
    await click(latchButton()!)
    expect(pushToast).toHaveBeenCalledWith('Transmit is held until this device signs in.', 'error')
  })

  it('Enter alone never arms; after the latch arms, Enter sends', async () => {
    await renderCw(false)
    await enterOnCall('K9AAA')
    expect(api.setTxEnabled, 'Enter never arms').not.toHaveBeenCalled()
    expect(api.sendCw, 'Enter never takes the self-arming send').not.toHaveBeenCalled()
    expect(api.sendCwArmed, 'TX off: Enter refuses before anything is sent').not.toHaveBeenCalled()

    await click(latchButton()!)
    expect(api.setTxEnabled.mock.calls).toEqual([[true]])
    expect(latchButton()?.textContent).toBe('TX On')

    fireEvent.keyDown(stripCall(), { key: 'Enter' })
    await flush()
    await flush()
    expect(api.sendCwArmed, 'armed: Enter sends').toHaveBeenCalledTimes(1)
    expect(api.sendCw).not.toHaveBeenCalled()
    expect(api.setTxEnabled.mock.calls, 'Enter armed nothing more').toEqual([[true]])
  })
})

describe('CONTROL: the CW TX latch at the station', () => {
  it('stays read-only', async () => {
    driving.value = false
    await renderCw(false)
    expect(latchButton()).toBeNull()
    expect(document.querySelector('.cockpit-txstrip .op-btn.monitor.readonly')?.textContent).toBe('TX Off')
  })
})
