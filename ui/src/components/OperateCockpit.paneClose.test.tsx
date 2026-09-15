// @vitest-environment jsdom
//
// THE PANE'S OWN ✕, COMPUTED — Operate (2026-09-15).
//
// The operator's report, in his words: on the FT8 screen he went looking for a way to close
// a panel and could not find one. Removal had shipped — ⊞ Panels unticks any of the eight
// entries in OPERATE_PANEL_IDS — but the affordance had not, so the capability may as well
// not have existed. Every removable pane now carries a ✕ in its own header.
//
// WHAT THIS FILE COMPUTES, and why each half is here:
//   1. EVERY listed pane that renders has a ✕, found by its ACCESSIBLE NAME ("Hide <the ⊞
//      label>") — the same words the menu entry uses, so a screen reader and the menu agree
//      about which panel this is.
//   2. Pressing it actually REMOVES the pane. This is the half a presence-only test cannot
//      see: a ✕ wired to `() => {}` renders, reads correctly, focuses correctly and does
//      nothing, and the whole suite stays green (that exact mutation is what the stop-line
//      sweeps' own "present, enabled and inert" caveat is about). So each case asserts the
//      pane is GONE afterwards AND that the ⊞ entry now reads unticked — the record moved,
//      not just the subtree.
//   3. Nothing else disappears with it. A ✕ that unmounted the wrong pane would pass (1) and
//      (2) and be a worse bug than the one this fixes.
//
// WHAT IT DOES NOT COMPUTE, stated rather than half-guarded:
//   · `waterfall` — its ✕ lives inside <Waterfall/>, which this file stubs (canvas). Swept in
//     Waterfall.paneClose.test.tsx instead, where the real component renders.
//   · `stations` — Classic's rail hosts the node App passes down as `roster` (a StationList),
//     so its ✕ and the act behind it are swept in StationList.paneClose.test.tsx. App's own
//     one line of wiring is not swept anywhere, and that is a human step.
//   · `txmeters` — Operate renders it into the QSO strip's telemetry cell, which has no head
//     of its own, so it has no ✕ and ⊞ Panels stays its only route. Asserted below as an
//     ABSENCE, so the day it grows one this file says so rather than staying quiet.
//   · That the ✕ survives a locale change: the accessible name is built from the same
//     `panelLabels()` the menu uses, so both move together or neither does.
//
// THE STOP LINE is untouched by all of this and this file makes no claim about it —
// OperateCockpit.structure.test.tsx owns that sweep. Nothing here adds an id, moves a
// control into a pane, or gates one: the ✕ only ever writes `removed` for an id the menu
// already listed.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup } from '../types'
import { OPERATE_PANEL_IDS } from '../features/panelState'
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

vi.mock('../api', async (importOriginal) => {
  // Auto-derived from the real module for the same reason stop-line.test.tsx derives its
  // mock: a hand-kept list omits whatever is added later, and the cockpit then THROWS on
  // mount at a seam the diff does not explain.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getLog: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => resolved),
    resolveEntity: vi.fn(async () => 'United States'),
    getSettings: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    getDeclination: vi.fn(async () => 0),
  }
})
// Canvas children only. The pane HEADS this file reads are all rendered by the cockpit
// itself or by OperateDecodes / StationList / RecallPanel / TxPanel, none of which is stubbed.
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
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
        calling: 'K9XYZ',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
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
}

/** A LIVE panel record — a real React state hook, the shape `usePanelLayout` has, because
 *  the whole point of these cases is that the ✕ WRITES and the cockpit then RE-READS. A
 *  `setPanelState: vi.fn()` stub turns every assertion below into a presence test wearing a
 *  click, which is exactly the blind spot this file exists to close. */
function useLivePanels(): PanelLayoutApi<OperatePanelId> {
  const [state, setState] = useState<Partial<Record<OperatePanelId, PanelState>>>({})
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => state[id] ?? 'docked',
    setPanelState: (id, s) => setState((cur) => ({ ...cur, [id]: s })),
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

const noop = () => {}

/** The last record any mounted host published, so a case can read what the ✕ actually wrote
 *  without reaching into React. */
let live: PanelLayoutApi<OperatePanelId> | null = null

function Host({ layout }: { layout: 'classic' | 'roster' }) {
  const panels = useLivePanels()
  live = panels
  return (
    <OperateCockpit
      snap={makeSnap()}
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
      onClearSelection={noop}
      layoutMode={layout}
      onLayoutMode={noop}
      panels={panels}
      active
    />
  )
}

function renderCockpit(layout: 'classic' | 'roster') {
  live = null
  return render(<Host layout={layout} />)
}

/** What the record says now — read AFTER the click, so it reports the write rather than the
 *  closure the button was built with. */
const stateOf = (id: OperatePanelId): PanelState => live?.stateOf(id) ?? 'docked'

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('Operate — every removable pane carries its own ✕', () => {
  // The panes this cockpit renders with a station selected, per layout, and the ⊞ label each
  // one is listed under. `waterfall` and `txmeters` are deliberately absent — see the header.
  const cases: Array<{ layout: 'classic' | 'roster'; id: OperatePanelId; label: string }> = [
    { layout: 'classic', id: 'bandActivity', label: 'Band Activity' },
    { layout: 'classic', id: 'rxfreq', label: 'Rx Frequency' },
    { layout: 'classic', id: 'txmsgs', label: 'Tx Messages' },
    { layout: 'classic', id: 'recall', label: 'Callsign card' },
    { layout: 'roster', id: 'callRoster', label: 'Call Roster' },
    { layout: 'roster', id: 'bandActivity', label: 'Band Activity' },
    { layout: 'roster', id: 'rxfreq', label: 'Rx Frequency' },
    { layout: 'roster', id: 'recall', label: 'Callsign card' },
  ]

  for (const { layout, id, label } of cases) {
    it(`${layout}: the ✕ on ${label} removes it and nothing else`, async () => {
      renderCockpit(layout)
      const close = await screen.findByRole('button', { name: `Hide ${label}` })
      expect(stateOf(id)).toBe('docked')
      fireEvent.click(close)
      // (2) THE RECORD MOVED. An inert ✕ — `onClick={() => {}}`, the mutation neither
      // stop-line sweep can see — dies here and nowhere else in the suite.
      expect(stateOf(id)).toBe('removed')
      // (3) …and only that entry. A ✕ wired to the wrong id would pass (1) and (2).
      for (const other of OPERATE_PANEL_IDS) {
        if (other !== id) expect(stateOf(other), other).toBe('docked')
      }
      // (1b) THE PANE IS GONE FROM THE SCREEN, not merely flagged: its own ✕ went with it.
      await waitFor(() => expect(screen.queryByRole('button', { name: `Hide ${label}` })).toBeNull())
    })
  }

  it('⊞ Panels shows the pane unticked afterwards — the menu is still the way back', async () => {
    renderCockpit('roster')
    fireEvent.click(await screen.findByRole('button', { name: 'Hide Band Activity' }))
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    // Found by the entry's own label, so this is the menu the operator reads, not the record.
    const entry = screen.getByLabelText('Band Activity') as HTMLInputElement
    expect(entry.checked).toBe(false)
    // Positive control: a pane nobody closed is still ticked, so an all-false menu (a broken
    // read) cannot pass this.
    expect((screen.getByLabelText('Rx Frequency') as HTMLInputElement).checked).toBe(true)
  })

  it('TX meters has NO ✕ — it renders into the QSO strip with no head of its own', async () => {
    renderCockpit('classic')
    // Control: the sweep CAN find a ✕ in this render, so an absence below is the panel and
    // not a broken query.
    expect(await screen.findByRole('button', { name: 'Hide Band Activity' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /^Hide TX Meters$/i })).toBeNull()
  })
})
