// @vitest-environment jsdom
//
// Rendered-structure guards for the DECODE-FIRST Classic rebuild (2026-08):
//   - the dock law as a rendered assertion: every protected TX control renders inside
//     the merged .cockpit-qso strip even with EVERY panel id 'removed';
//   - Band Activity and the promoted Rx Frequency pane receive the SAME click-model
//     function identities (the decodeClickProps spread) — a future fork of the click
//     model goes red here instead of shipping two divergent click behaviours;
//   - the Rx Frequency pane rides the .cockpit-qsocol column with the Tx1–Tx6 machine
//     beneath it (WSJT-X bottom-right geometry);
//   - TX meters are a fixed strip cell, not a permanent body row, and the cell exists
//     in BOTH RX and TX states (zero mount/unmount with the 15 s cycle).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import { OPERATE_PANEL_IDS } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'
import * as OD from './OperateDecodes'

vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))

vi.mock('../api', () => {
  const nothing = () => Promise.resolve(null)
  return {
    getSettings: vi.fn(() => Promise.resolve({})),
    setSettings: vi.fn(nothing),
    openPanelWindow: vi.fn(nothing),
    notifyErase: vi.fn(nothing),
    pointRotatorAtCall: vi.fn(nothing),
    redecode: vi.fn(nothing),
    startCq: vi.fn(nothing),
    startQsoRecording: vi.fn(nothing),
    stopQsoRecording: vi.fn(nothing),
    setSkipTx1: vi.fn(nothing),
    getDeclination: vi.fn(nothing),
    getSatTrackStatus: vi.fn(nothing),
    readRotator: vi.fn(nothing),
    stopRotator: vi.fn(nothing),
    stopSatTrack: vi.fn(nothing),
    openQrzPage: vi.fn(nothing),
    postSpot: vi.fn(nothing),
    setFrequency: vi.fn(nothing),
    setRit: vi.fn(nothing),
    setXit: vi.fn(nothing),
    setVfo: vi.fn(nothing),
    getSpectrumRow: vi.fn(nothing),
    setDecodeDepth: vi.fn(nothing),
  }
})

// Capture every OperateDecodes render's props so prop IDENTITY can be asserted —
// the factory owns the array (vi.mock hoists above module-scope lets).
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  const captured: Array<Record<string, unknown>> = []
  const OperateDecodes = (props: Record<string, unknown>) => {
    captured.push(props)
    return <div data-testid="od-pane" data-title={String(props.title ?? 'Band Activity')} />
  }
  return { ...real, OperateDecodes, __captured: captured }
})
const captured = (OD as unknown as { __captured: Array<Record<string, unknown>> }).__captured

function makeSnap(over: { transmitting?: boolean } = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
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
      transmitting: over.transmitting ?? false,
      tuning: false,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(state: Partial<Record<OperatePanelId, PanelState>>): PanelLayoutApi<OperatePanelId> {
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => state[id] ?? 'docked',
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    reset: vi.fn(),
  }
}

function renderCockpit(
  state: Partial<Record<OperatePanelId, PanelState>>,
  over: { transmitting?: boolean; layoutMode?: 'classic' | 'roster' } = {},
) {
  const noop = () => {}
  const onCall = vi.fn()
  const view = render(
    <OperateCockpit
      snap={makeSnap(over)}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={onCall}
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
      layoutMode={over.layoutMode ?? 'classic'}
      onLayoutMode={noop}
      panels={panelsApi(state)}
      active={false}
    />,
  )
  return { ...view, onCall }
}

beforeEach(() => {
  captured.length = 0
})
afterEach(() => cleanup())

const PROTECTED = [
  /call cq/i,
  /s&p/i,
  /tx off|tx on/i,
  /^tune$/i,
  /stop tx/i,
  /hold tx$/i,
  /tx auto/i,
  /skip tx1/i,
]

/** Every id in the REAL vocabulary, removed. Built from OPERATE_PANEL_IDS rather than
 *  written out, so an id added to the vocabulary is swept by this guard the moment it
 *  exists — the hand-written object this replaces would have left a new id untested and
 *  still looked exhaustive. */
const ALL_REMOVED: Partial<Record<OperatePanelId, PanelState>> = Object.fromEntries(
  OPERATE_PANEL_IDS.map((id) => [id, 'removed' as PanelState]),
)

describe('the merged operating strip is the un-removable TX surface', () => {
  // This is Operate's half of THE STOP LINE (features/panelState.ts): the wiring check that
  // no panel id gates a control which stops a transmission. The other four cockpits are
  // swept in components/stop-line.test.tsx; Operate is here because its stop controls live
  // in the merged QSO strip rather than a CockpitHeader, and this suite already owns the
  // mock surface for them.
  it('every protected control renders INSIDE .cockpit-qso with every panel id removed', () => {
    const { container } = renderCockpit(ALL_REMOVED)
    for (const name of PROTECTED) {
      const btn = screen.getByRole('button', { name })
      expect(btn, String(name)).toBeTruthy()
      expect(
        btn.closest('.cockpit-qso'),
        `${String(name)} rendered OUTSIDE the merged strip — the dock law says the strip is ` +
          'the one un-removable host for TX/sequencer controls',
      ).not.toBeNull()
    }
    // The next-slot countdown moved into the strip with the period controls.
    const next = screen.getByText(/next \d+s/)
    expect(next.closest('.cockpit-qso')).not.toBeNull()
    // The old status row is gone as a DOM element, not just restyled.
    expect(container.querySelector('.cockpit-status')).toBeNull()
  })

  it('the strip carries the TX-state cap (the ▲ TRANSMITTING pulse lives here now)', () => {
    renderCockpit({}, { transmitting: true })
    const cap = screen.getByText('▲ TRANSMITTING')
    expect(cap.closest('.cockpit-qso')).not.toBeNull()
  })
})

describe('both decode panes share one click model (prop identity, never a fork)', () => {
  it('Band Activity and Rx Frequency receive the SAME handler identities', () => {
    const { onCall } = renderCockpit({})
    // Re-renders (the async settings fetch) push again — assert on the LAST render of
    // each pane role, not an exact call count.
    const rx = [...captured].reverse().find((p) => p.lockedFilter === 'rx')
    const band = [...captured].reverse().find((p) => p.lockedFilter === undefined)
    expect(rx, 'no Rx-Frequency pane rendered').toBeTruthy()
    expect(band, 'no Band Activity pane rendered').toBeTruthy()
    // Identity, not shape: the decodeClickProps spread is shared, so select/tune/ignore
    // and the double-click work-station path CANNOT diverge between the panes.
    expect(rx!.onSelectDecode).toBe(band!.onSelectDecode)
    expect(rx!.onSetRx).toBe(band!.onSetRx)
    expect(rx!.onToggleIgnore).toBe(band!.onToggleIgnore)
    expect(rx!.onCall).toBe(band!.onCall)
    // …and the work-station path is the cockpit's onCall prop — no new sequencing path.
    expect(rx!.onCall).toBe(onCall)
    expect(rx!.compact).toBe(true)
  })
})

describe('the country exclusion stops at the chase list', () => {
  // Band Activity is "who is on the band that I want to work" — the right place to thin
  // countries out. Rx Frequency is "what is happening on MY frequency": an excluded-country
  // station sitting on top of us is exactly what we must see, and hiding it would make our
  // own frequency read as clear when it is not. Pinned in BOTH layouts, because the Rx pane
  // is mounted twice and one of the two silently gaining the filter is the drift this file
  // exists to catch.
  it.each(['classic', 'roster'] as const)('%s: Rx Frequency opts out, Band Activity does not', (layoutMode) => {
    renderCockpit({}, { layoutMode })
    const rx = [...captured].reverse().find((p) => p.lockedFilter === 'rx')
    const band = [...captured].reverse().find((p) => p.lockedFilter === undefined)
    expect(rx, 'no Rx-Frequency pane rendered').toBeTruthy()
    expect(band, 'no Band Activity pane rendered').toBeTruthy()
    expect(rx!.hideExcludedCountries).toBe(false)
    // Left at its default (on) rather than passed explicitly — a pane that must filter
    // should not depend on a call site remembering to ask for it.
    expect(band!.hideExcludedCountries).toBeUndefined()
  })
})

describe('the promoted Rx-Frequency column (WSJT-X bottom-right geometry)', () => {
  it('the Rx pane rides .cockpit-qsocol with the Tx1–Tx6 machine beneath it', () => {
    const { container } = renderCockpit({})
    const qsocol = container.querySelector('.cockpit-qsocol')
    expect(qsocol, 'no .cockpit-qsocol column — the Rx pane is still a rail strip').not.toBeNull()
    const rx = qsocol!.querySelector('.cockpit-rxfreq')
    expect(rx, 'the Rx Frequency pane is not inside the qsocol').not.toBeNull()
    const tx = qsocol!.querySelector('.tx-panel')
    expect(tx, 'the Tx1–Tx6 machine is not inside the qsocol').not.toBeNull()
    // Rx pane above, Tx machine below — the WSJT-X geometry the operator runs a QSO from.
    expect(
      rx!.compareDocumentPosition(tx!) & Node.DOCUMENT_POSITION_FOLLOWING,
      'the Tx machine renders ABOVE the Rx pane',
    ).toBeTruthy()
    // The Stations roster is the third column, alone in the aside.
    const aside = container.querySelector('aside.cockpit-side')
    expect(aside).not.toBeNull()
    expect(aside!.querySelector('[data-testid="stations-roster"]')).not.toBeNull()
    expect(aside!.querySelector('.cockpit-rxfreq')).toBeNull()
    // All three columns populated → the grid says so.
    expect(container.querySelector('.cockpit-lower.classic')?.getAttribute('data-cols')).toBe('three')
  })
})

describe('the column seam exists only where the grid consumes its vars', () => {
  // Only the three-column template consumes --op-col-a/-b. In the two-column
  // collapse (Band Activity removed) a rendered seam is a DEAD drag: it paints
  // vars the template ignores while onCommit silently rewrites the persisted
  // txmsgs/stations shares, reshaping the 3-column layout for later.
  it("data-cols='three': the qsocol carries the drag seam", () => {
    const { container } = renderCockpit({})
    expect(container.querySelector('.cockpit-lower.classic')?.getAttribute('data-cols')).toBe('three')
    expect(container.querySelector('.cockpit-qsocol .pane-splitter.col-seam')).not.toBeNull()
  })

  it("data-cols='two' (Band Activity removed): no seam renders", () => {
    const { container } = renderCockpit({ bandActivity: 'removed' })
    expect(container.querySelector('.cockpit-lower.classic')?.getAttribute('data-cols')).toBe('two')
    expect(
      container.querySelector('.pane-splitter.col-seam'),
      'a column seam rendered against the 2-track template — its drag is dead on screen ' +
        'but still rewrites the persisted column shares',
    ).toBeNull()
  })
})

describe('TX meters: a fixed strip cell, never a body row', () => {
  it('no permanent .ph-txmeters row under .cockpit-body; the cell sits in the strip', () => {
    const { container } = renderCockpit({})
    expect(
      container.querySelector('.cockpit-body > .ph-txmeters'),
      'the TX-meters placeholder row is back as a body child — the dead row the operator flagged',
    ).toBeNull()
    expect(container.querySelector('.cockpit-qso .cq-telemetry .ph-txmeters')).not.toBeNull()
  })

  it('the cell exists in BOTH RX and TX states — the 15 s cycle never mounts/unmounts it', () => {
    const rx = renderCockpit({})
    expect(rx.container.querySelector('.cq-telemetry')).not.toBeNull()
    cleanup()
    const tx = renderCockpit({}, { transmitting: true })
    expect(tx.container.querySelector('.cq-telemetry')).not.toBeNull()
  })

  it("removing the 'txmeters' panel removes the cell entirely", () => {
    const { container } = renderCockpit({ txmeters: 'removed' })
    expect(container.querySelector('.cq-telemetry')).toBeNull()
  })
})
