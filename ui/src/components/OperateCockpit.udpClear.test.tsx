// @vitest-environment jsdom
//
// A LOGGER'S CLEAR ERASES THE PANES ITS WINDOW NAMES, as WSJT-X's does (`++window`; `& 1` Band
// Activity, `& 2` Rx Frequency; widgets/mainwindow.cpp:667-674). The engine counts a Clear of each
// pane on its own tick — `clearTick` for Band Activity, `rxFreqClearTick` for Rx Frequency — so a
// Clear of Rx Frequency only no longer empties Band Activity as well. Each layout draws the two
// panes at its own site, so each is swept: Classic, Roster, and the arranged columns.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, DecodeRow } from '../types'
import { OPERATE_PANELS, usePanelLayout } from '../features/panelState'
import type { OperateLayout, OperatePanelId, PanelLayoutApi } from '../features/panelState'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module, as the other FT suites do: a hand-kept list omits whatever is
  // added later and the cockpit throws on mount.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    getDeclination: vi.fn(async () => 0),
    resolveEntity: vi.fn(async () => 'United States'),
  }
})
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const decode = (over: Partial<DecodeRow>): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1500,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

// One decode on the Rx frequency (both panes show it) and one away from it (Band Activity only).
const DECODES: DecodeRow[] = [
  decode({}),
  decode({ from: 'K1ABC', message: 'CQ K1ABC FN42', freqHz: 800 }),
]

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  stations: [],
  recentDecodes: DECODES,
  conversations: [],
  highlights: [],
  harqRescues: 0,
  clearTick: 0,
  rxFreqClearTick: 0,
  qso: null,
  link: { tier: 'FT8', periodSecs: 15 },
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
    decodeDepth: 2,
  },
} as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

let api: PanelLayoutApi<OperatePanelId> | null = null
const noop = () => {}

function Live({ layoutMode, ticks }: { layoutMode: OperateLayout; ticks: { clearTick: number; rxFreqClearTick: number } }) {
  const panels = usePanelLayout(OPERATE_PANELS)
  api = panels
  return (
    <OperateCockpit
      snap={{ ...snap, ...ticks } as AppSnapshot}
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
      selectedCall={null}
      onSelect={noop}
      layoutMode={layoutMode}
      onLayoutMode={noop}
      panels={panels}
      active
    />
  )
}

/** The rows each pane shows, as [Band Activity, Rx Frequency]. */
function rows(): [number, number] {
  const count = (grip: string) => {
    const pane = document.querySelector(`[data-pane-grip="${grip}"]`)?.closest('section.operate-decodes')
    expect(pane, `the ${grip} pane is on screen`).toBeTruthy()
    return pane!.querySelectorAll('.decode-row').length
  }
  return [count('bandActivity'), count('rxfreq')]
}

type Site = 'classic' | 'roster' | 'arranged'

/** The cockpit at `site` with both panes holding their decodes, and a way to send the next ticks. */
function mount(site: Site) {
  const layoutMode: OperateLayout = site === 'classic' ? 'classic' : 'roster'
  let ticks = { clearTick: 0, rxFreqClearTick: 0 }
  const view = render(<Live layoutMode={layoutMode} ticks={ticks} />)
  if (site === 'arranged') {
    // A pane moved: the cockpit draws the arranged columns, its third site for the two panes.
    act(() => api!.movePane!('rxfreq', 'left', () => true, false, { layout: 'roster' }))
    expect(document.querySelector('.cockpit-lower [data-pane="rxfreq"]'), 'the columns are arranged').toBeTruthy()
  }
  expect(rows(), 'premise: [Band Activity, Rx Frequency] rows').toEqual([2, 1])
  return (next: Partial<typeof ticks>) => {
    ticks = { ...ticks, ...next }
    view.rerender(<Live layoutMode={layoutMode} ticks={ticks} />)
  }
}

describe('a logger’s Clear erases only the panes its window names', () => {
  for (const site of ['classic', 'roster', 'arranged'] as const) {
    it(`${site}: Clear of Rx Frequency only empties Rx Frequency; Band Activity keeps its rows`, () => {
      const send = mount(site)
      send({ rxFreqClearTick: 1 })
      expect(rows()).toEqual([2, 0])
    })

    it(`${site}: Clear of Band Activity empties Band Activity; Rx Frequency keeps its row`, () => {
      const send = mount(site)
      send({ clearTick: 1 })
      expect(rows()).toEqual([0, 1])
    })

    it(`${site}: Clear of both empties both (CONTROL)`, () => {
      const send = mount(site)
      send({ clearTick: 1, rxFreqClearTick: 1 })
      expect(rows()).toEqual([0, 0])
    })
  }
})
