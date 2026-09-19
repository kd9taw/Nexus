// @vitest-environment jsdom
//
// ONE READ OF THE LOG PER WINDOW, NOT ONE PER VIEW (the big-log fix, 2026-09-19).
//
// A user's 150k-contact log stopped Nexus working. One full read of it is ~100 MB of JSON, and
// the main window made one per reader: the three log strips the RTTY, PSK and JS8 cockpits keep
// mounted (hidden) from startup each read it for themselves, and the Operate callsign card read
// it again on every selection and every logged contact. This renders those REAL readers — three
// LogEntry strips and the OperateCockpit's card — against an engine that counts what it hands
// over, and pins the two numbers the fix is for: ONE whole-log transfer at startup, and ONE row
// (no whole-log transfer) when a contact is logged. Run RED before the fix (4 transfers at
// startup; a logged contact re-read the whole log and never reached the strips), GREEN after.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, within, fireEvent, screen } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, LoggedQso } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

/** The engine's side: its log, and a tally of what each read handed over. */
const engine = vi.hoisted(() => ({
  log: [] as unknown[],
  revision: 1,
  /** Reads that handed over the WHOLE log. */
  wholeLog: 0,
  /** Rows handed over by each read that did not. */
  deltaRows: [] as number[],
}))

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module, so an export added later cannot make a reader
  // throw on mount here; the reads under test are overridden below.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return {
    ...auto,
    // The whole log — what every reader called before the fix.
    getLog: vi.fn(async () => {
      engine.wholeLog++
      return engine.log.slice()
    }),
    // An append-only engine: a copy it has seen gets the rows after it; a first read gets it all.
    getLogDelta: vi.fn(async (sinceRevision: number, haveCount: number) => {
      if (sinceRevision === 0) {
        engine.wholeLog++
        return { revision: engine.revision, full: true, rows: engine.log.slice() }
      }
      const rows = engine.log.slice(haveCount)
      engine.deltaRows.push(rows.length)
      return { revision: engine.revision, full: false, rows }
    }),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => 'United States'),
    searchParks: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({})),
  }
})
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

// Two prior contacts with W1ABC, both off the live band.
const priorQsos = [
  { call: 'W1ABC', country: 'United States', grid: 'FN31', band: '40m', freqMhz: 7.074, mode: 'FT8',
    rstSent: '-08', rstRcvd: '-12', whenUnix: Date.UTC(2026, 2, 14) / 1000, confirmed: true },
  { call: 'W1ABC', country: 'United States', grid: 'FN31', band: '15m', freqMhz: 21.074, mode: 'FT4',
    rstSent: '-15', rstRcvd: '-11', whenUnix: Date.UTC(2025, 10, 2) / 1000, confirmed: false },
] as unknown as LoggedQso[]
/** The contact the sequencer files while W1ABC is still on the card. */
const justLogged = {
  call: 'W1ABC', country: 'United States', grid: 'FN31', band: '20m', freqMhz: 14.074, mode: 'FT8',
  rstSent: '-10', rstRcvd: '-09', whenUnix: Date.UTC(2026, 8, 19) / 1000, confirmed: false,
} as unknown as LoggedQso

/** The snapshot every reader in the window gets; a logged contact moves both ticks, as the engine does. */
function makeSnap(tick: number): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    logTick: tick,
    loggedTick: tick,
    hunt: null,
    stations: [
      { call: 'W1ABC', grid: 'FN42', snr: -7, lastHeardSlot: 0, heardCount: 3, presence: 'live', worked: true, country: 'United States' },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074, band: '20m', sideband: 'USB', slot: 0, source: 'native', sourceLabel: 'Native',
      nextSlotMs: 5000, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true,
      txEnabled: false, txAllowed: true, transmitting: false, tuning: false, atu: true, qsoRecording: false,
      catOk: true, splitTxMhz: null,
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

/** The main window's log readers: the three strips the hidden cockpits keep mounted, and the
 *  Operate cockpit with W1ABC selected (its callsign card). */
function Window({ tick }: { tick: number }) {
  const snap = makeSnap(tick)
  const noop = () => {}
  return (
    <>
      {(['RTTY', 'PSK31', 'JS8'] as const).map((mode) => (
        <div key={mode} data-testid={`strip-${mode}`}>
          <LogEntry snap={snap} mode={mode} defaultRst="599" exchange="terrestrial" titled={false} />
        </div>
      ))}
      <div data-testid="operate">
        <OperateCockpit
          snap={snap}
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
        />
      </div>
    </>
  )
}

/** Prior contacts a reader's callsign card lists (0 when it shows none). */
function listed(testId: string): number {
  const list = screen.getByTestId(testId).querySelector('.recall-log-list')
  return list ? within(list as HTMLElement).getAllByRole('listitem').length : 0
}
/** Let every read that is going to happen, happen. */
const settle = () => new Promise((r) => setTimeout(r, 100))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  engine.log = [...priorQsos]
  engine.revision = 1
  engine.wholeLog = 0
  engine.deltaRows = []
})
afterEach(cleanup)

describe('the main window reads the log once, not once per reader', () => {
  it('three log strips and the Operate card cost ONE whole-log read at startup', async () => {
    render(<Window tick={1} />)
    await waitFor(() => expect(listed('operate')).toBe(2))
    await settle()
    expect(engine.wholeLog, 'whole-log reads at startup').toBe(1)
  })

  it('a logged contact moves ONE row, and reaches the Operate card and a strip alike', async () => {
    const { rerender } = render(<Window tick={1} />)
    // The same station typed into a strip, so that strip's card lists its history too.
    fireEvent.change(within(screen.getByTestId('strip-RTTY')).getByPlaceholderText('Call'), {
      target: { value: 'W1ABC' },
    })
    await waitFor(() => expect(listed('operate')).toBe(2))
    await waitFor(() => expect(listed('strip-RTTY')).toBe(2))
    await settle()
    const before = { whole: engine.wholeLog, deltas: engine.deltaRows.length }

    // The sequencer files W1ABC on 20 m in the background; the next snapshot carries the tick.
    engine.log = [...engine.log, justLogged]
    engine.revision++
    rerender(<Window tick={2} />)
    await waitFor(() => expect(listed('operate'), 'the Operate card never showed the logged contact').toBe(3))
    await waitFor(() => expect(listed('strip-RTTY'), 'the strip never showed the logged contact').toBe(3))
    await settle()
    expect(engine.wholeLog - before.whole, 'whole-log reads for one logged contact').toBe(0)
    expect(engine.deltaRows.slice(before.deltas), 'rows moved per read').toEqual([1])
  })
})
