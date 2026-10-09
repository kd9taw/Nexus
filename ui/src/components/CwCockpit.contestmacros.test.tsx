// @vitest-environment jsdom
//
// THE CW MACRO SET IN A CONTEST THAT IS NOT FIELD DAY.
//
// The cockpit had two built-in sets: casual, and Field Day. Whenever ANY contest was running
// it used the Field Day one, so `F1` in the Illinois QSO Party or CQ WW CW keyed
// `CQ FD DE …` — Field Day's own call, on the air, in somebody else's contest. There is now a
// third set: the same contest cadence with the universal contest call, `CQ TEST`.
//
// Field Day's set is unchanged, and that is asserted here rather than assumed: it is what its
// operators have been keying for releases.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CwCockpit } from './CwCockpit'
import { isFieldDay } from '../fdEvent'
import type { AppSnapshot, FieldDayStatus } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "keys CQ TEST in another contest, CQ FD in Field Day…", takes
// 0.29 s and 0.28 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
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

/** What the mount-time `getSettings` resolves with. Mutable per test, like `decodeState` —
 *  the cockpit reads `rigModel` from here to decide whether the CAT-keying caution applies. */
const settingsState = {
  macros: { cwProfiles: [] as unknown[], activeCwProfile: 0 },
  rigModel: 0,
}
/** The backend's "CAT CW keying is unproven on this model" list. Empty = rule unread. */
let unprovenModels: number[] = []

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => settingsState),
  getCatCwUnprovenRigModels: vi.fn(async () => unprovenModels),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))

vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  decodeState.sent = []
  decodeState.keyerError = null
  settingsState.macros.cwProfiles = []
  settingsState.rigModel = 0
  unprovenModels = []
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

async function renderCockpit(props: Partial<Parameters<typeof CwCockpit>[0]> = {}) {
  const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)
  // Let the mount-time getSettings / cwDecode / previewCw promises land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}


const FD = { event: 'arrlfd', running: false } as unknown as FieldDayStatus
const ILQP = { event: 'ilqp', running: false } as unknown as FieldDayStatus

/** The label and key of each macro button the dock renders. */
const macroRows = () =>
  [...document.querySelectorAll('.cw-macro')].map((b) => ({
    key: b.querySelector('.cw-macro-key')?.textContent ?? '',
    label: b.querySelector('.cw-macro-label')?.textContent ?? '',
  }))

describe('the CW macro set follows the contest that is running', () => {
  it('keys CQ TEST in another contest, CQ FD in Field Day, and CQ outside a contest', async () => {
    await renderCockpit({ fieldDay: ILQP })
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ TEST' })
    // The rest of the contest cadence is Field Day's, which is what a contest needs.
    expect(macroRows().map((m) => m.key)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
    expect(macroRows()[2].label).toBe('Exch')

    // POSITIVE CONTROLS: Field Day still says CQ FD, and no contest still says CQ.
    cleanup()
    await renderCockpit({ fieldDay: FD })
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ FD' })
    cleanup()
    await renderCockpit()
    expect(macroRows()[0]).toEqual({ key: 'F1', label: 'CQ' })
  })

  it('sends the contest call, not Field Day\'s', async () => {
    const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
    await renderCockpit({ fieldDay: ILQP })
    await act(async () => {
      fireEvent.click([...document.querySelectorAll('.cw-macro')][0])
    })
    expect(api.sendCw).toHaveBeenCalledWith('CQ TEST DE {MYCALL} {MYCALL} K')
    // POSITIVE CONTROL: Field Day sends its own.
    api.sendCw.mockClear()
    cleanup()
    await renderCockpit({ fieldDay: FD })
    await act(async () => {
      fireEvent.click([...document.querySelectorAll('.cw-macro')][0])
    })
    expect(api.sendCw).toHaveBeenCalledWith('CQ FD DE {MYCALL} {MYCALL} K')
  })
})

// ---------------------------------------------------------------------------
// THE REPORT IN A CONTEST EXCHANGE.
//
// `{EXCH}` is the running contest's exchange WITHOUT the signal report, which `{RST}` keys
// (`Engine::contest_sent_exchange`). The contest set's F3 and F4 carried `{EXCH}` and no
// `{RST}`, so in the Illinois QSO Party, whose exchange is "RS(T) and county", F3 keyed the
// county with no 5NN. Where the exchange has no report (Sweepstakes, the California QSO Party,
// the ARRL VHF contests) a 5NN is a wrong exchange — Sweepstakes would copy it as the serial —
// so those keep the set without it.
//
// Every shipped ruleset is checked: its received slots are read off the rules seed the engine
// loads, as the engine tags them, and its answer is what its sponsor's rules say. A ruleset
// added to the seed with no entry below fails until somebody says which it is. The strings
// these templates key through the real engine, contest by contest, are pinned on the Rust side
// (`cw_contest_macros_key_each_built_in_contests_exchange`).
// ---------------------------------------------------------------------------

type SeedRuleset = {
  event: string
  exchange: { fields: { key: string; kind: { type: string } }[]; roles: { id: string; receives: string[] }[] }
}
const SEED = JSON.parse(
  readFileSync(resolve(process.cwd(), '../crates/tempo-core/src/fd_rules.seed.json'), 'utf8'),
) as { rulesets: SeedRuleset[] }

/** Does the sponsor's exchange carry a signal report? From each contest's own rules. */
const EXCHANGE_HAS_REPORT: Record<string, boolean> = {
  ilqp: true, // RS(T), and county, state, province or DX
  tnqp: true, // RS(T), and county or state
  ohqp: true, // RST, and county, state or DX
  txqp: true, // RS(T), and county or state
  nyqp: true, // RS(T), and county or state
  cqww_cw: true, // RST and CQ zone
  cqww_ssb: true,
  cqww_rtty: true, // RST, CQ zone, and a W/VE station's state
  cqwpx_cw: true, // RST and serial
  cqwpx_ssb: true,
  arrlss_cw: false, // serial, precedence, call, check, section
  arrlss_ssb: false,
  cqp: false, // serial, and county, state or DX
  arrlvhf_jan: false, // grid
  arrlvhf_jun: false,
  arrlvhf_sep: false,
}

/** `dto::field_kind_tag`: the seed's `one_of` is the wire's `oneOf`; every other tag is spelled
 *  the same. */
const wireKind = (type: string) => (type === 'one_of' ? 'oneOf' : type)

/** The contest block the engine serialises for a ruleset, as far as the macro choice reads it:
 *  the event and the received slots of each of its roles. */
function fieldDayFor(r: SeedRuleset, role: number): FieldDayStatus {
  return {
    event: r.event,
    running: true,
    role: r.exchange.roles[role].id,
    receives: r.exchange.roles[role].receives.map((key) => ({
      key,
      kind: wireKind(r.exchange.fields.find((f) => f.key === key)!.kind.type),
      required: true,
    })),
  } as unknown as FieldDayStatus
}

const REPORT = { F3: '! DE {MYCALL} {RST} {EXCH} {EXCH} K', F4: '! TU {RST} {EXCH} DE {MYCALL} K' }
const NO_REPORT = { F3: '! DE {MYCALL} {EXCH} {EXCH} K', F4: '! TU {EXCH} DE {MYCALL} K' }

/** What F3 and then F4 hand the keyer, pressed on the dock. */
async function f3f4(): Promise<string[]> {
  const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>
  api.sendCw.mockClear()
  for (const i of [2, 3]) {
    await act(async () => {
      fireEvent.click([...document.querySelectorAll('.cw-macro')][i])
    })
  }
  return api.sendCw.mock.calls.map((c) => c[0] as string)
}

const contests = SEED.rulesets.filter((r) => !isFieldDay(r.event))

describe('F3 and F4 key the report where the contest exchange carries one', () => {
  it('reads every shipped contest that is not a Field Day', () => {
    expect(contests.map((r) => r.event).sort()).toEqual(Object.keys(EXCHANGE_HAS_REPORT).sort())
  })

  it.each(contests.flatMap((r) => r.exchange.roles.map((role, i) => [`${r.event} (${role.id || 'all'})`, r, i] as const)))(
    '%s',
    async (_name, r, role) => {
      await renderCockpit({ fieldDay: fieldDayFor(r, role) })
      const want = EXCHANGE_HAS_REPORT[r.event] ? REPORT : NO_REPORT
      expect(await f3f4()).toEqual([want.F3, want.F4])
    },
  )

  it('leaves Field Day, no contest, and an operator\'s own macros as they were', async () => {
    // Field Day's exchange (class and section) has no report, and its set is unchanged.
    for (const event of ['arrlfd', 'wfd']) {
      await renderCockpit({ fieldDay: { event, running: true } as unknown as FieldDayStatus })
      expect(await f3f4()).toEqual([NO_REPORT.F3, NO_REPORT.F4])
      cleanup()
    }
    // A casual QSO sends its report with F3 already.
    await renderCockpit()
    expect(await f3f4()).toEqual(['! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN', '! DE {MYCALL} TU 73 SK'])
    cleanup()
    // An operator's saved profile is theirs: the Illinois QSO Party keys it as written.
    settingsState.macros.cwProfiles = [
      {
        name: 'Mine',
        macros: [
          { key: 'F1', label: 'CQ', text: 'CQ IL {MYCALL}' },
          { key: 'F2', label: 'Call', text: '{MYCALL}' },
          { key: 'F3', label: 'Exch', text: '! {EXCH}' },
          { key: 'F4', label: 'TU', text: 'TU {EXCH}' },
        ],
      },
    ]
    const ilqp = SEED.rulesets.find((r) => r.event === 'ilqp')!
    await renderCockpit({ fieldDay: fieldDayFor(ilqp, 0) })
    expect(await f3f4()).toEqual(['! {EXCH}', 'TU {EXCH}'])
  })
})

// ---------------------------------------------------------------------------
// THE SIGNED CONTEST LAYOUT IS NOT ON THE AIR YET.
//
// Enter Sends Message's layout for these sets (F2 his call and the exchange, F3 TU, …) is signed,
// and it goes on the air WITH Enter Sends Message, never before: `CW_LAYOUT_ON_AIR` in
// `features/esmRoles.ts` is the one switch. Until it is on, F3 keys today's exchange in the
// contest sets and at Field Day, as their operators have keyed it for releases. Switching it on
// turns this red, and the change that does so updates this test with it.
// ---------------------------------------------------------------------------

describe('the signed contest layout is not on the air yet', () => {
  it('F3 still sends today\'s exchange in a contest with a report, one without, and at Field Day', async () => {
    const f3 = async () => (await f3f4())[0]
    await renderCockpit({ fieldDay: fieldDayFor(SEED.rulesets.find((r) => r.event === 'ilqp')!, 0) })
    expect(await f3()).toBe('! DE {MYCALL} {RST} {EXCH} {EXCH} K')
    cleanup()
    await renderCockpit({ fieldDay: fieldDayFor(SEED.rulesets.find((r) => r.event === 'arrlss_cw')!, 0) })
    expect(await f3()).toBe('! DE {MYCALL} {EXCH} {EXCH} K')
    cleanup()
    await renderCockpit({ fieldDay: { event: 'arrlfd', running: true } as unknown as FieldDayStatus })
    expect(await f3()).toBe('! DE {MYCALL} {EXCH} {EXCH} K')
  })
})
