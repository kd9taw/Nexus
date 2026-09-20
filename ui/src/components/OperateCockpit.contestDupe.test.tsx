// @vitest-environment jsdom
//
// THE CONTEST-SCOPED DUPE BADGE ON THE FT COCKPIT'S CALLSIGN CARD.
//
// During an FT contest run from Operate no surface showed a contest-scoped dupe at all.
// Operate hosts no LogEntry, and LogEntry's FD strip is where the contest verdict lived, so
// the only dupe cue an FT contest operator got was the recall card's lifetime `Dupe {band}`
// badge — "worked on this band, ever". That badge is correct for general operating and it
// lights for a 2019 QSO that is a perfectly fresh contest contact.
//
// So this file's load-bearing test is not "a badge appears". It is THE DISCRIMINATION: on one
// station, on one card, the lifetime badge and the contest badge must be able to disagree,
// each answering its own question. A test that only asserted the contest badge shows up when
// the station is a contest dupe would pass just as well on a badge wired to the general log —
// which is the bug.
//
// The lifetime badge is asserted present throughout, deliberately. It is the positive control
// for every negative: "no contest badge" only means something on a card that is demonstrably
// rendering badges at all, and the operator's ruling was that the lifetime badge STAYS (it is
// what keeps this card agreeing with the solid B4 chip the roster and decode feed show for the
// same station from `worked_band_set`).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, FieldDayStatus, LoggedQso, QrzLookup } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const resolved: QrzLookup = {
  call: 'W1ABC',
  name: 'Alice Example',
  nickname: null,
  qth: 'Hartford, CT',
  grid: 'FN31',
  state: 'CT',
  country: 'United States',
  dxcc: 291,
  cqZone: 5,
  ituZone: 8,
  image: null,
}

/** ⭐ THE GENERAL LOG: W1ABC worked on 20m — the LIVE band — but in 2019, long before any
 *  contest. This is the contact that makes the lifetime badge light and that must NOT make
 *  the contest badge light. It is the whole reason the second badge exists. */
const priorQsos = [
  {
    call: 'W1ABC',
    country: 'United States',
    grid: 'FN31',
    band: '20m',
    freqMhz: 14.074,
    mode: 'FT8',
    rstSent: '-08',
    rstRcvd: '-12',
    whenUnix: Date.UTC(2019, 5, 1) / 1000,
    confirmed: false,
  },
] as unknown as LoggedQso[]

const qrzLookup = vi.fn(async () => resolved)
const getLog = vi.fn(async () => priorQsos)

vi.mock('../api', () => ({
  getLog: (...a: unknown[]) => getLog(...(a as [])),
  getLogDelta: async () => ({ revision: 1, full: true, rows: await getLog() }),
  qrzLookup: (...a: unknown[]) => qrzLookup(...(a as [])),
  resolveEntity: vi.fn(async () => 'United States'),
  getSettings: vi.fn(() => Promise.resolve({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  startCq: vi.fn(async () => null),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => null),
  postSpot: vi.fn(async () => null),
  setFrequency: vi.fn(async () => null),
  setRit: vi.fn(async () => null),
  setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null),
  getSpectrumRow: vi.fn(async () => null),
  setDecodeDepth: vi.fn(async () => null),
  atuTune: vi.fn(async () => null),
  setMsk144Period: vi.fn(async () => null),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

/** A contest session. `log` is THIS position's contest log, `club.dupes` other positions'. */
const contest = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: '',
    qsoCount: 1,
    log: [],
    ...over,
  }) as unknown as FieldDayStatus

/** W1ABC already worked IN THE CONTEST, on the live band and this cockpit's mode class. */
const workedInContest = contest({
  log: [{ call: 'W1ABC', class: '1D', section: 'CT', band: '20m', mode: 'DIG', submode: 'FT8' }],
} as unknown as Partial<FieldDayStatus>)

function makeSnap(fieldDay: FieldDayStatus | null): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    loggedTick: 0,
    logTick: 0,
    mygrid: 'EN61',
    stations: [
      {
        call: 'W1ABC',
        grid: 'FN42',
        snr: -7,
        lastHeardSlot: 0,
        heardCount: 3,
        presence: 'live',
        worked: true,
        country: 'United States',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    fieldDay,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074,
      band: '20m',
      sideband: 'USB',
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      nextSlotMs: 5000,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      txEven: true,
      txCycleAuto: true,
      txEnabled: false,
      txAllowed: true,
      transmitting: false,
      tuning: false,
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<OperatePanelId, PanelState>> = {}
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => state[id] ?? 'docked',
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

function renderCockpit(fieldDay: FieldDayStatus | null, fdActive: boolean) {
  const noop = () => {}
  return render(
    <OperateCockpit
      snap={makeSnap(fieldDay)}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall="W1ABC"
      onSelect={noop}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active={false}
      fdActive={fdActive}
    />,
  )
}

/** The card, once the debounced callbook answer has landed. */
async function card(): Promise<HTMLElement> {
  await waitFor(() => expect(document.querySelector('.recall-card')).not.toBeNull())
  return document.querySelector('.recall-card') as HTMLElement
}

/** The badge row's chips, by the class that carries each verdict. */
function badges(c: HTMLElement) {
  const text = (sel: string) => c.querySelector(sel)?.textContent?.trim() ?? null
  return {
    lifetime: text('.recall-badge.dupe'),
    contest: text('.recall-badge.contest-dupe'),
    club: text('.recall-badge.club-dupe'),
  }
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  qrzLookup.mockClear()
  qrzLookup.mockResolvedValue(resolved)
  getLog.mockClear()
})
afterEach(cleanup)

describe('the FT cockpit card carries a contest-scoped dupe badge', () => {
  it('shows it when the station is already in the contest log on this band and mode class', async () => {
    renderCockpit(workedInContest, true)
    const c = await card()
    await waitFor(() => expect(c.querySelector('.recall-badge.contest-dupe')).not.toBeNull())
    expect(badges(c).contest).toBe('Contest dupe')
  })

  // ⭐ THE ONE THAT MATTERS. Same station, same card, same instant: worked on 20m in 2019, not
  // worked in this contest. The lifetime badge must say so and the contest badge must not — a
  // contest badge wired to the general log (the bug) fails exactly here and nowhere else.
  it('does NOT show it for a lifetime dupe that is a fresh contest contact — the two disagree', async () => {
    renderCockpit(contest(), true)
    const c = await card()
    // The positive control: the card IS rendering a dupe verdict, from the 2019 contact.
    await waitFor(() => expect(c.querySelector('.recall-badge.dupe')).not.toBeNull())
    const b = badges(c)
    expect(b.lifetime, 'the lifetime badge must still light for the 2019 QSO').toBe('Dupe 20m')
    expect(b.contest, 'a fresh contest contact must not read as a contest dupe').toBeNull()
  })

  it('keeps BOTH when the station is a dupe in both senses — neither replaces the other', async () => {
    renderCockpit(workedInContest, true)
    const c = await card()
    await waitFor(() => expect(c.querySelector('.recall-badge.contest-dupe')).not.toBeNull())
    const b = badges(c)
    expect(b.contest).toBe('Contest dupe')
    expect(b.lifetime, 'the lifetime badge was explicitly kept, not replaced').toBe('Dupe 20m')
  })

  it('reads differently for a CLUB key — a warning, not the hard block', async () => {
    renderCockpit(
      contest({ club: { syncState: 'synced', dupes: [['W1ABC', '20m', 'DIG']], board: [] } } as unknown as Partial<FieldDayStatus>),
      true,
    )
    const c = await card()
    await waitFor(() => expect(c.querySelector('.recall-badge.club-dupe')).not.toBeNull())
    const b = badges(c)
    expect(b.club).toBe('Club dupe')
    expect(b.contest, 'a club key is not the own-log hard block').toBeNull()
  })
})

describe('the contest badge is scoped to a running contest', () => {
  // The gate, both halves. The SAME contest log that lights the badge above must light nothing
  // with the contest switched off — otherwise the badge is not contest-scoped at all, it is
  // just a second reading of a snapshot field that happens to be present.
  it('stays dark with the contest switched off, on the very log that lights it', async () => {
    renderCockpit(workedInContest, false)
    const c = await card()
    await waitFor(() => expect(c.querySelector('.recall-badge.dupe')).not.toBeNull())
    const b = badges(c)
    expect(b.contest, 'fdActive=false must clear the badge').toBeNull()
    expect(b.club).toBeNull()
    expect(b.lifetime, 'general operating is unchanged').toBe('Dupe 20m')
  })

  it('stays dark when no contest rides the snapshot at all', async () => {
    renderCockpit(null, true)
    const c = await card()
    await waitFor(() => expect(c.querySelector('.recall-badge.dupe')).not.toBeNull())
    expect(badges(c).contest).toBeNull()
  })
})
