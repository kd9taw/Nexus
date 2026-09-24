// @vitest-environment jsdom
//
// WHICH QUESTION EACH LOG VIEW ASKS — AND WHEN IT ASKS NONE (SPEC-2 v3 C17b, V3).
//
// The log strip (LogEntry, seven cockpits) and the Operate callsign card used to compute their
// badges with `callHistory(allLog, call, band, mode, matchMode)`, `isNewEntity(allLog, entity)` and
// `entitySlots(allLog, entity)` over the whole log. They now ask `LogSource` the same thing. The
// answers are pinned elsewhere (the goldens; the per-view suites). What only a wiring test can see
// is the QUESTION: the arguments the old calls took, field for field, and the gates — a strip in a
// running contest, or on a Remote browser, never read the general log, and must still ask nothing.
// A recording source around the real adapter lists every question a view puts to it.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { OperateCockpit } from './OperateCockpit'
import { setLogSource, type LogSource } from '../features/logSource'
import { wholeLogSource } from '../features/wholeLogSource'
import type { LogQuestion } from '../features/logAnswers'
import type { AppSnapshot, FieldDayStatus, LoggedQso } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  return {
    ...auto,
    getLogDelta: vi.fn(async () => ({ revision: 1, full: true, rows: LOG })),
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

const LOG = [
  { call: 'W1ABC', country: 'United States', band: '40m', freqMhz: 7.03, mode: 'CW', whenUnix: 1_700_000_000, confirmed: true, awardConfirmed: false },
] as unknown as LoggedQso[]

/** Every question put to the real adapter, in order. */
const asked: LogQuestion[] = []
const recording: LogSource = {
  ...wholeLogSource,
  peek: (q) => {
    asked.push(q)
    return wholeLogSource.peek(q)
  },
}
const lastOf = <K extends LogQuestion['kind']>(kind: K) =>
  [...asked].reverse().find((q) => q.kind === kind) as Extract<LogQuestion, { kind: K }> | undefined
const logQuestions = () => asked.filter((q) => q.kind === 'callHistory' || q.kind === 'entity')

const snapWith = (over: Record<string, unknown> = {}) =>
  ({
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    logTick: 1,
    b4MatchMode: true,
    hunt: null,
    stations: [{ call: 'W1ABC', grid: 'FN42', snr: -7, lastHeardSlot: 0, heardCount: 3, presence: 'live', worked: true, country: 'United States' }],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 7.03, band: '40m', sideband: 'USB', slot: 0, source: 'native', sourceLabel: 'Native',
      nextSlotMs: 5000, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5, txEven: true, txCycleAuto: true,
      txEnabled: false, txAllowed: true, transmitting: false, tuning: false, atu: true, qsoRecording: false,
      catOk: true, splitTxMhz: null,
    },
    ...over,
  }) as unknown as AppSnapshot

beforeEach(() => {
  asked.length = 0
  setLogSource(recording)
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the log strip asks exactly what it used to compute', () => {
  it('a typed call: its history on the rig’s band, in the strip’s mode, under the operator’s dupe scope', async () => {
    render(<LogEntry snap={snapWith()} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1abc' } })
    await waitFor(() => expect(lastOf('entity')?.entity).toBe('United States'))
    // The strip upper-cases what is typed (`logCall`) — the value the old call was handed.
    expect(lastOf('callHistory')).toEqual({ kind: 'callHistory', call: 'W1ABC', band: '40m', mode: 'CW', matchMode: true })
    // …and the answer reached the card: the one prior contact.
    await waitFor(() => expect(document.querySelectorAll('.recall-log-list [role="listitem"]').length).toBe(1))
  })

  it('without the operator’s dupe-scope setting, the scope is call + band (matchMode false)', async () => {
    render(<LogEntry snap={snapWith({ b4MatchMode: undefined })} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />)
    fireEvent.change(screen.getByPlaceholderText('Call'), { target: { value: 'w1abc' } })
    await waitFor(() => expect(lastOf('callHistory')?.call).toBe('W1ABC'))
    expect(lastOf('callHistory')?.matchMode).toBe(false)
  })

  it('in a running contest the strip asks the general log nothing (the contest log decides dupes)', async () => {
    const fieldDay = { running: true, state: '', qsoCount: 0, sections: 0, points: 0, log: [], composing: [] } as unknown as FieldDayStatus
    render(<LogEntry snap={snapWith()} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} fieldDay={fieldDay} fdMode="CW" />)
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20))
    })
    expect(logQuestions()).toEqual([])
  })

  it('on a Remote browser the strip asks the general log nothing', async () => {
    const remote = { submit: vi.fn(), recall: () => null } as unknown as Parameters<typeof LogEntry>[0]['remote']
    render(<LogEntry snap={snapWith()} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} remote={remote} />)
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20))
    })
    expect(logQuestions()).toEqual([])
  })
})

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

describe('the Operate callsign card asks exactly what it used to compute', () => {
  it('the selected call, upper-cased, on the rig’s band, in the tier’s mode', async () => {
    const noop = () => {}
    render(
      <OperateCockpit
        snap={snapWith()}
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
      />,
    )
    await waitFor(() => expect(lastOf('entity')?.entity).toBe('United States'))
    expect(lastOf('callHistory')).toEqual({ kind: 'callHistory', call: 'W1ABC', band: '40m', mode: 'FT8', matchMode: true })
    await waitFor(() => expect(document.querySelectorAll('.recall-log-list [role="listitem"]').length).toBe(1))
  })
})
