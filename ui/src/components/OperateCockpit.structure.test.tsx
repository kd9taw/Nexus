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
import { render, screen, cleanup, within } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import { OPERATE_PANEL_IDS, OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'
import * as OD from './OperateDecodes'

vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))
// The RF scope pane's picture (PhoneScope, through the spectrum renderer) is a canvas jsdom cannot
// draw; its frame and its place in the strip are what this file checks.
vi.mock('./PhoneScope', () => ({
  PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-canvas" data-feed={p.feed} />,
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

function makeSnap(over: { transmitting?: boolean; atu?: boolean | null } = {}): AppSnapshot {
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
      // A rig WITH a tuner by default, so the dock-law sweep covers the ATU button;
      // `atu: null` models a tuner-less rig (the button must then not render at all).
      atu: over.atu === undefined ? true : over.atu,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(state: Partial<Record<OperatePanelId, PanelState>>): PanelLayoutApi<OperatePanelId> {
  const layout = { v: 1 as const, state, share: {} }
  return {
    layout,
    // The vocabulary's own reading of an absent entry: docked, but for the panes it ships hidden
    // (the RF scope pane), exactly as App's record reads it.
    stateOf: (id) => panelStateIn(OPERATE_PANELS, layout, id),
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

function cockpitElement(
  state: Partial<Record<OperatePanelId, PanelState>>,
  over: {
    transmitting?: boolean
    atu?: boolean | null
    layoutMode?: 'classic' | 'roster'
    fdActive?: boolean
    fdRuleset?: import('../api').FdRulesetDto | null
  } = {},
) {
  const noop = () => {}
  const onCall = vi.fn()
  const element = (
    <OperateCockpit
      snap={makeSnap(over)}
      theme="dark"
      tier="FT8"
      fdActive={over.fdActive ?? false}
      fdRuleset={over.fdRuleset ?? null}
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
    />
  )
  return { element, onCall }
}

function renderCockpit(
  state: Partial<Record<OperatePanelId, PanelState>>,
  over: Parameters<typeof cockpitElement>[1] = {},
) {
  const { element, onCall } = cockpitElement(state, over)
  const view = render(element)
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
  /^atu$/i,
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

// ── THE WATERFALL STRIP, BOTH STATES (operator, 2026-08-16) ─────────────────────────────
// Operate needed no code change for the ask — `waterfall` has had a ⊞ entry since 0.15.0,
// and it is the pattern the other four cockpits were built to on 2026-08-16. What was
// missing is this: the behaviour was covered only incidentally, by the all-ids-removed
// sweep below, which asserts what SURVIVES a hide and never that the hide happened. Pinned
// here so the five cockpits are checked the same way, and so a regression in the one that
// shipped first cannot hide behind the four that came after.
describe('the waterfall strip is hideable, and shown until the operator says otherwise', () => {
  it('renders on a stock layout (nothing stored ⇒ docked)', () => {
    renderCockpit({})
    expect(document.querySelector('.cockpit-waterfall'), 'the strip went missing on a stock layout').not.toBeNull()
    expect(document.querySelector('[data-testid="waterfall-canvas"]')).not.toBeNull()
  })

  it("⊞ 'Waterfall' unticked takes the strip and its splitter, and the decode lists stay", () => {
    renderCockpit({ waterfall: 'removed' })
    expect(document.querySelector('.cockpit-waterfall'), 'the strip survived its own hide').toBeNull()
    expect(document.querySelector('[data-testid="waterfall-canvas"]')).toBeNull()
    // 'removed' is not 'popped': no re-dock bar, because there is no window to come back from.
    expect(document.querySelector('.wf-redock'), 'a removal offered a re-dock bar').toBeNull()
    // What the height is freed FOR.
    expect(document.querySelector('.cockpit-body'), 'the body went with the strip').not.toBeNull()
  })

  it("'popped' is a THIRD state and still leaves the re-dock bar", () => {
    // The distinction the shared `scope` id in the other four cockpits deliberately does not
    // have — and the reason Operate keeps its own `waterfall` id rather than being renamed
    // into the shared one (features/panelState.ts, SCOPE_PANEL_ID).
    renderCockpit({ waterfall: 'popped' })
    expect(document.querySelector('.cockpit-waterfall')).toBeNull()
    expect(document.querySelector('.wf-redock'), 'a popped-out waterfall left no way back').not.toBeNull()
  })
})

// ── THE RF SCOPE PANE BESIDE THE WATERFALL (operator, 2026-10-03: an opt-in pane) ────────────────
// It ships hidden; ticked, it stands beside the waterfall in the strip (`.cockpit-rfbeside`, a row:
// cockpit-panes.css), so it takes width from the waterfall and no height from the decode lists or
// the QSO strip; and adding it never re-parents the waterfall.
describe('the RF scope pane: hidden until ticked, then beside the waterfall in its strip', () => {
  it('a stock layout is the waterfall alone, the strip a column as before', () => {
    const { container } = renderCockpit({})
    const strip = container.querySelector('.cockpit-waterfall')
    expect(strip, 'the strip went missing').not.toBeNull()
    expect(container.querySelector('[data-pane="rfScope"]'), 'the pane shipped visible').toBeNull()
    expect(strip!.classList.contains('cockpit-rfbeside')).toBe(false)
    expect(strip!.children.length).toBe(1)
  })

  it('ticked, it stands beside the waterfall in the strip, drawn as the RF feed', () => {
    const { container } = renderCockpit({ rfScope: 'docked' })
    const strip = container.querySelector('.cockpit-waterfall')!
    expect(strip.classList.contains('cockpit-rfbeside'), 'the strip did not turn into a row').toBe(true)
    // The waterfall stays the strip's FIRST child (`.cockpit-waterfall > :first-child` sizes it).
    expect(strip.firstElementChild?.getAttribute('data-testid')).toBe('waterfall-canvas')
    const pane = strip.querySelector('[data-pane="rfScope"]')
    expect(pane, 'the pane is not in the strip').not.toBeNull()
    expect(pane!.getAttribute('aria-label')).toBe('RF scope')
    expect(pane!.querySelector('[data-testid="rfscope-canvas"]')?.getAttribute('data-feed')).toBe('rf')
    // Its ✕ is the same act as the tick, and nothing in it transmits or stops.
    expect(within(pane as HTMLElement).queryAllByRole('button').map((b) => b.getAttribute('aria-label') ?? b.textContent)).toEqual([
      expect.stringMatching(/RF scope/),
    ])
    // The QSO strip with Stop TX is outside the waterfall strip, untouched.
    expect(strip.querySelector('.cockpit-qso')).toBeNull()
    expect(container.querySelector('.cockpit-qso')).not.toBeNull()
  })

  it('with the waterfall hidden or popped out, the pane has the strip to itself', () => {
    for (const waterfall of ['removed', 'popped'] as const) {
      const { container } = renderCockpit({ waterfall, rfScope: 'docked' })
      const strip = container.querySelector('.cockpit-waterfall')
      expect(strip, `${waterfall}: the pane lost its strip`).not.toBeNull()
      expect(strip!.classList.contains('cockpit-rfbeside'), `${waterfall}: a row of one`).toBe(false)
      expect(strip!.querySelector('[data-testid="waterfall-canvas"]')).toBeNull()
      expect(strip!.querySelector('[data-pane="rfScope"]')).not.toBeNull()
      expect(container.querySelector('.wf-redock') != null, `${waterfall}: the re-dock bar`).toBe(waterfall === 'popped')
      cleanup()
    }
  })

  it('ticking it never remounts the waterfall (the strip gains a sibling, nothing is re-parented)', () => {
    const view = renderCockpit({})
    const before = view.container.querySelector('[data-testid="waterfall-canvas"]')
    expect(before).not.toBeNull()
    view.rerender(cockpitElement({ rfScope: 'docked' }).element)
    expect(view.container.querySelector('[data-pane="rfScope"]')).not.toBeNull()
    expect(view.container.querySelector('[data-testid="waterfall-canvas"]'), 'the waterfall was remounted').toBe(before)
  })

  it('the ⊞ menu does not count it as something the operator hid', () => {
    renderCockpit({})
    const menu = screen.getByRole('button', { name: /⊞ Panels/ })
    expect(menu.textContent ?? '', 'the stock layout reads as "1 hidden"').not.toMatch(/hidden/i)
    cleanup()
    // Control: a pane the operator DID hide is counted.
    renderCockpit({ bandActivity: 'removed' })
    expect(screen.getByRole('button', { name: /⊞ Panels/ }).textContent ?? '').toMatch(/1 hidden/)
  })
})

describe('the merged operating strip is the un-removable TX surface', () => {
  // This is Operate's half of THE STOP LINE (features/panelState.ts): the wiring check that
  // at least one control which stops a transmission renders OUTSIDE every ⊞-removable pane,
  // so no panel id can gate it. The other four cockpits are swept in
  // components/stop-line.test.tsx; Operate is here because its stop controls live in the
  // merged QSO strip rather than a CockpitHeader, and this suite already owns the mock
  // surface for them. Asserting they render inside `.cockpit-qso` is the direct form of
  // "outside every pane" — the strip is not a CockpitPaneFrame and has no vocabulary id.
  //
  // PROTECTED BELOW IS NOT A STOP-CONTROL LIST, and must not be read as Operate's census. It
  // is the whole TX/sequencer surface of the strip — the DOCK LAW, a wider claim than the stop
  // line. Of the eight, exactly one stops an over in flight: Stop TX (→ onHaltTx →
  // Engine::halt_tx). Tune ends its own carrier. TX On/Off and S&P are NOT stop controls —
  // `set_tx_enabled` deliberately does not arm `slot_tx_abort` (operator 2026-07-31: the FT
  // over in flight completes; the button's own tooltip says so), and `onSetMode('qso-monitor')`
  // builds a fresh monitoring station with running:false, ending the CQ RUN and dropping the
  // queue without arming anything. Call CQ, Hold Tx, TX auto and Skip Tx1 are here for the dock
  // law alone. Operate's third census holder, Esc (window keydown → the same halt), is
  // keyboard-only and unsweepable here.
  //
  // No Operate pane hosts a stop control of its own (the two that do in the app are Phone's
  // `voiceKeyer` and RTTY's `stream`). Five of Operate's seven panes are SENDERS and all five
  // are hideable — the rule is indifferent.
  //
  // IT IS WEAKER THAN THAT FILE'S SWEEP AND IS NOT ITS EQUIVALENT. This is PRESENCE-ONLY:
  // every id removed at once, no baseline capture, no `disabled` comparison, no one-id-at-a-
  // time pass. It catches a control that VANISHES with a hide; it would not catch one left
  // mounted and disabled, nor one taken out by a single id while surviving the full sweep.
  // Bringing it up to the four-cockpit shape means rendering Operate's real strip against a
  // nothing-hidden baseline — worth doing, not done here, and not claimed.
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

  // …AND OVER OPERATE'S LAYOUTS (layout L3's stop-line ruling for Operate). Operate has no pane
  // placement; what it arranges is its two layouts and the side its rail stands on (layout L5). Each
  // of the four, with nothing hidden and with every id hidden: the whole surface still in the strip.
  // Presence-only, like the sweep above.
  it.each([
    ['classic', 'right'],
    ['classic', 'left'],
    ['roster', 'right'],
    ['roster', 'left'],
  ] as const)('%s layout, rail on the %s: every protected control is in the strip, hidden panes or not', (layoutMode, side) => {
    for (const state of [{}, ALL_REMOVED]) {
      localStorage.setItem('nexus.operate.railSide', side)
      // In a `finally`: a red here must not leave the rail on the left for the tests after it.
      try {
        const { container } = renderCockpit(state, { layoutMode })
        // The layout and the side really took (the side with the rail on screen), or this would sweep
        // one layout four times.
        expect(container.querySelector('.cockpit-lower')?.classList.contains(layoutMode), `${layoutMode}: the layout did not apply`).toBe(true)
        if (Object.keys(state).length === 0)
          expect(container.querySelector('.cockpit-lower')?.getAttribute('data-rail') ?? 'right', `${layoutMode}: the rail side did not apply`).toBe(side)
        for (const name of PROTECTED) {
          const btn = screen.getByRole('button', { name })
          expect(btn.closest('.cockpit-qso'), `${layoutMode}, rail ${side}: ${String(name)} left the strip`).not.toBeNull()
        }
      } finally {
        cleanup()
        localStorage.removeItem('nexus.operate.railSide')
      }
    }
  })

  it('the strip carries the TX-state cap (the ▲ TRANSMITTING pulse lives here now)', () => {
    renderCockpit({}, { transmitting: true })
    const cap = screen.getByText('▲ TRANSMITTING')
    expect(cap.closest('.cockpit-qso')).not.toBeNull()
  })

  it('a rig with no tuner gets NO ATU button (an ATU button on a radio with no ATU is worse than no button)', () => {
    renderCockpit({}, { atu: null })
    expect(screen.queryByRole('button', { name: /^atu$/i })).toBeNull()
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

describe('the column dividers exist only where the grid consumes their tokens', () => {
  // Only Classic's three-column template consumes --op-col-ba/-a/-b, and only Roster's two-column
  // one --op-roster-a/-b. In a collapse (a column removed) a rendered divider is a DEAD drag: it
  // paints tokens the template ignores while its commit silently rewrites the stored widths,
  // reshaping the layout for later. Each divider is the GRID's own child (layout L5: absolutely
  // positioned on the gap before its track), never a column's, whose clip would cut it.
  const seams = (c: HTMLElement) => [...c.querySelectorAll('.pane-splitter.col-seam')]
  it("Classic data-cols='three': two dividers, children of the grid, on the gaps before tracks 2 and 3", () => {
    const { container } = renderCockpit({})
    const grid = container.querySelector('.cockpit-lower.classic')!
    expect(grid.getAttribute('data-cols')).toBe('three')
    expect(seams(container).map((d) => d.parentElement)).toEqual([grid, grid])
    expect(seams(container).map((d) => [...d.classList].filter((k) => k.startsWith('op-colseam')))).toEqual([
      ['op-colseam', 'op-colseam-2'],
      ['op-colseam', 'op-colseam-3'],
    ])
    expect(seams(container).map((d) => d.getAttribute('aria-label'))).toEqual([
      'Band Activity / Rx Frequency column',
      'Rx Frequency column / Stations roster',
    ])
  })

  it("Classic data-cols='two' (Band Activity removed): no divider renders", () => {
    const { container } = renderCockpit({ bandActivity: 'removed' })
    expect(container.querySelector('.cockpit-lower.classic')?.getAttribute('data-cols')).toBe('two')
    expect(
      seams(container),
      'a column divider rendered against the 2-track template — its drag is dead on screen ' +
        'but still rewrites the stored column widths',
    ).toEqual([])
  })

  it("Roster data-cols='two': one divider, between the Call Roster and the side rail; none at one column", () => {
    const { container } = renderCockpit({}, { layoutMode: 'roster' })
    const grid = container.querySelector('.cockpit-lower.roster')!
    expect(seams(container).map((d) => [d.parentElement, d.getAttribute('aria-label')])).toEqual([[grid, 'Call Roster / side rail']])
    cleanup()
    const alone = renderCockpit({ callRoster: 'removed' }, { layoutMode: 'roster' })
    expect(alone.container.querySelector('.cockpit-lower.roster')?.getAttribute('data-cols')).toBe('one')
    expect(seams(alone.container)).toEqual([])
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

describe('the warn-only FD banned-mode chip in the header (a status line, never a control)', () => {
  const WFD: import('../api').FdRulesetDto = {
    event: 'wfd',
    rulesYear: 2026,
    bannedModes: ['FST4', 'FT4', 'FT8', 'JT4', 'JT9', 'JT65', 'Q65', 'MSK144', 'WSPR', 'FST4W', 'ECHO'],
    spottingAllowed: true,
    clusterAllowed: true,
    enforcement: 'warn',
  }

  it('renders in the header for a banned tier (FT8 at WFD), outside every ⊞-removable pane', () => {
    // EVERY vocabulary id removed: the chip lives in the header shell child, so
    // the screen that remains still carries it — and it is a passive div, so the
    // stop-line census is untouched.
    const { container } = renderCockpit(ALL_REMOVED, { fdActive: true, fdRuleset: WFD })
    const chip = container.querySelector('.cockpit-header .fd-advisory.banned')
    expect(chip, 'FT8 at WFD must warn in the Operate header').not.toBeNull()
    expect(chip!.textContent).toContain('Winter Field Day')
    expect(chip!.closest('button, a, [role="button"]'), 'the chip must be passive').toBeNull()
  })

  it('absent with FD off — the stock header is unchanged', () => {
    const { container } = renderCockpit({})
    expect(container.querySelector('.fd-advisory')).toBeNull()
  })
})
