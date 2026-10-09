// @vitest-environment jsdom
//
// PHONE COCKPIT SHELL STRUCTURE (2026-07-30 layout assessment, design3 §3/§5).
//
// The shell contract has five child kinds: header chrome, the scope, the TX strip under it, ONE
// pane region (.cockpit-panes, tier-stamped by useRegionCols) and the pinned TX dock — and since
// 2026-10-03 Phone may show a sixth, THE LEFT SIDE, beside the scope, the strip and the region.
// So in the DOM the scope, the strip and the region stand in a STAGE inside the left side's ROW,
// two wrappers that are always there and generate no box until the side shows (cockpit-panes.css
// "THE LEFT SIDE"); the header and the dock are shell children. These tests pin the parts of that
// contract that live in TSX, where no CSS test can see them:
//   - every operator-content block renders through a CockpitPaneFrame inside the region,
//   - the PTT row renders in .cockpit-txdock and NEVER inside a pane (a pane scrolls;
//     the control that keys the rig must not),
//   - the ⊞ Panels 'removed' gating still hides exactly the pane it names,
//   - the column grouping follows the tier: 1/2 → band+keyer+aux | log, 3 → band+keyer
//     | aux | log, with maxCols capped when a column would have nothing to hold. (The
//     keyer keeps the LEADING column at every tier — a tier-dependent column would
//     remount it, and its unmount cleanup aborts an in-flight voice TX; see the
//     remount tests below.)
//
// Heavy children (scope canvas, header readout, the pane internals) are stubbed — this
// suite asserts the SHELL's structure, not the panes' behaviour, which keeps it honest
// about what it can see in jsdom (no layout; widths are stubbed like useRegionCols.test).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS, PHONE_PANELS, panelStorageKey, usePanelLayout } from '../features/panelState'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "hiding ANY panel in the vocabulary leaves every…", takes 0.36 s
// and 0.36 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one
// core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it.
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))

// Structure-irrelevant heavy children → stubs. The stubs keep a testid so "the pane's
// content is inside its frame" stays assertable.
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
// The stub reports `titled` so this suite can see the ONE prop that is a placement decision
// rather than log behaviour: whether the strip draws its own heading under a frame head that
// already says LOG. The strip's own half is in LogEntry.density.test.tsx.
vi.mock('./LogEntry', () => ({
  LogEntry: (p: { titled?: boolean }) => (
    <div data-testid="log-stub" data-titled={String(p.titled ?? true)} />
  ),
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
// The two boards Phone hosts as feeds (#345). Stubbed like every other pane's content: this
// suite asserts WHERE they sit and that tier flips keep them, PhoneCockpit.boards.test.tsx what
// they show.
vi.mock('./SpotsPanel', () => ({ SpotsPanel: () => <div data-testid="spots-stub" /> }))
vi.mock('./NeededPanel', () => ({ NeededPanel: () => <div data-testid="needed-stub" /> }))

/** Fire a resize the way the browser does: EVERY live observer hears it (the useRegionCols.test.tsx
 *  harness kept only the last one created, and a split divider's own observer — the feeds' pair
 *  at tier 2 — then silently took the region's place). */
let fire: (() => void) | null = null
beforeEach(() => {
  const live = new Set<() => void>()
  fire = () => [...live].forEach((cb) => cb())
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      live.add(cb)
    }
    observe() {}
    disconnect() {
      live.delete(this.cb)
    }
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

/** jsdom has no layout: clientWidth is 0 unless stubbed. */
function stubWidth(el: Element, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}

/** Flush the hook's rAF debounce. */
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}

/** Snapshot with a CAT rig that reports NB/NR + NR-level/AGC, so the DSP and RX-DSP-levels
 *  aux panes render (the rigscope pane needs a live scope feed, which the stubbed
 *  PhoneScope never reports — deliberately out of scope here). */
function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

function fakePanels(removed: PhonePanelId[] = []): PanelLayoutApi<PhonePanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

/** A record with panes on the LEFT SIDE (2026-10-03). */
function sidePanels(leftSide: PhonePanelId[], removed: PhonePanelId[] = []): PanelLayoutApi<PhonePanelId> {
  return { ...fakePanels(removed), layout: { v: 2, state: {}, share: {}, leftSide } }
}

/** The effective window width useViewport publishes on <html> (the side shows from 1280). */
function windowWidth(px: number | null) {
  if (px == null) document.documentElement.style.removeProperty('--vw-eff')
  else document.documentElement.style.setProperty('--vw-eff', `${px}px`)
}
afterEach(() => windowWidth(null))

const renderCockpit = (props: Partial<Parameters<typeof PhoneCockpit>[0]> = {}) =>
  render(<PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)

describe('PhoneCockpit pane-grid shell', () => {
  it('the shell holds no child kinds beyond the contract (design3 §5 rule 1)', () => {
    // The recurrence-proof leans on this: "making X unreachable would require adding a
    // shell-level sibling, which fails contract test 1" — so the test has to exist. The
    // sanctioned shell children are header chrome, the left side's ROW (holding the stage, and
    // the side when it shows), ONE TX dock, and modal chrome (SpotDialog portals in as
    // .logconfirm-backdrop when open; mocked null here). The stage holds the scope (+ its
    // divider), the TX strip and ONE pane region. Anything else is a new sibling and must update
    // this census — deliberately, with a name — not slip in.
    renderCockpit({ snap: makeSnap({ transmitting: true, txSwr: 1.2 }) })
    const shell = document.querySelector('main.layout.single.phone-cockpit')!
    const SHELL = ['.cockpit-header', '.cockpit-flat', '.cockpit-leftrow', '.cockpit-txdock', '.logconfirm-backdrop']
    for (const el of Array.from(shell.children)) {
      expect(
        SHELL.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
    // The row, then the dock: the dock is the shell's LAST box, full width, whatever the row holds.
    const row = shell.querySelector(':scope > .cockpit-flat, :scope > .cockpit-leftrow')!
    expect(row, 'no left-side row in the shell').not.toBeNull()
    expect(row.nextElementSibling?.matches('.cockpit-txdock'), 'the dock does not follow the row').toBe(true)
    // With no side shown, the row holds the stage alone, and both are the box-less kind.
    expect(row.matches('.cockpit-flat')).toBe(true)
    expect([...row.children].map((c) => c.className)).toEqual(['cockpit-flat'])
    const stage = row.children[0]
    for (const el of Array.from(stage.children)) {
      expect(
        ['.ph-scope-panel', '.pane-splitter', '.cockpit-txstrip', '.cockpit-panes'].some((s) => el.matches(s)),
        `unexpected stage child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(stage.querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(document.querySelectorAll('.cockpit-panes').length).toBe(1)
    // The scope's divider gives back its column's gap from the stage (styles.css `.in-column`;
    // cockpit-shells.test.ts computes the net in both states).
    expect(stage.querySelector(':scope > .pane-splitter')!.classList.contains('in-column')).toBe(true)
    // With no side the scope is not size-contained (styles.css `.ph-scope-panel--beside`).
    expect(stage.querySelector(':scope > .ph-scope-panel')!.classList.contains('ph-scope-panel--beside')).toBe(false)
    // THE TX STRIP (2026-10-01): exactly one, directly under the scope (after its divider), holding
    // the stop controls the header used to hold — and the header none.
    const strips = stage.querySelectorAll(':scope > .cockpit-txstrip')
    expect(strips.length, 'no TX strip in the stage').toBe(1)
    expect(document.querySelectorAll('.cockpit-txstrip').length).toBe(1)
    expect(strips[0].previousElementSibling?.matches('.pane-splitter'), 'the TX strip is not directly under the scope').toBe(true)
    const named = (root: Element, re: RegExp) => [...root.querySelectorAll('button')].filter((b) => re.test(b.textContent!.trim()))
    expect(named(strips[0], /^stop tx$/i).length, 'Stop TX is not in the TX strip').toBe(1)
    expect(named(strips[0], /^tune$/i).length, 'Tune is not in the TX strip').toBe(1)
    expect(named(shell.querySelector('.cockpit-header')!, /^stop tx$|^tune$|^tuning…$|^atu$|tx (on|off)$/i), 'the header still draws a transmit control').toEqual([])
  })

  it('renders exactly one .cockpit-panes region, tier-stamped by useRegionCols', () => {
    renderCockpit()
    const regions = document.querySelectorAll('.cockpit-panes')
    expect(regions.length).toBe(1)
    // jsdom width 0 → the hook keeps its initial tier 1, stamped before first paint.
    expect(regions[0].getAttribute('data-cols')).toBe('1')
    // Tier 1 renders the two-column grouping (it stacks — the region is the scroller).
    expect(regions[0].querySelectorAll(':scope > .cockpit-col').length).toBe(2)
  })

  it('every operator-content block renders through a CockpitPaneFrame inside the region', () => {
    renderCockpit()
    for (const id of ['bandActivity', 'voiceKeyer', 'log', 'receiver', 'transmitter']) {
      const pane = document.querySelector(`[data-pane="${id}"]`)
      expect(pane, `pane "${id}" missing`).not.toBeNull()
      expect(pane!.classList.contains('pane-frame'), `"${id}" is not a .pane-frame`).toBe(true)
      expect(pane!.closest('.cockpit-panes'), `"${id}" renders outside the region`).not.toBeNull()
    }
    // The frames actually contain their content (not empty shells beside it).
    expect(document.querySelector('[data-pane="voiceKeyer"] [data-testid="vk-stub"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="log"] [data-testid="log-stub"]')).not.toBeNull()
    expect(
      document.querySelector('[data-pane="bandActivity"] [data-testid="bandstrip-stub"]'),
    ).not.toBeNull()
  })

  it('the log pane tells the strip the frame already titles it', () => {
    // The frame head reads LOG and is the pane's accessible name; the strip's own
    // "Log this QSO" said it again ~30px above a Log button that was already below the
    // fold at the default window. `titled` defaults TRUE, so a host that stops passing it
    // silently gets the duplicate back — hence the assertion here and not in the strip.
    renderCockpit()
    const stub = document.querySelector('[data-pane="log"] [data-testid="log-stub"]')!
    expect(stub.getAttribute('data-titled')).toBe('false')
  })

  it('PTT lives in the pinned TX dock — never inside a pane or the region', () => {
    renderCockpit()
    const ptt = document.querySelector('.ph-ptt')
    expect(ptt, 'no PTT button').not.toBeNull()
    expect(ptt!.closest('.cockpit-txdock'), 'PTT is not in the TX dock').not.toBeNull()
    expect(ptt!.closest('.pane-frame'), 'PTT is inside a pane frame — a pane can scroll it away').toBeNull()
    expect(ptt!.closest('.cockpit-panes'), 'PTT is inside the pane region').toBeNull()
    // The dock holds no pane frames at all. Not because "TX chrome has no id" — the dock also
    // renders TxMeters, and `txmeters` IS in PHONE_PANEL_IDS — but because nothing in the dock
    // renders THROUGH CockpitPaneFrame, so no pane scroller can carry PTT out of reach.
    const dock = document.querySelector('.cockpit-txdock')!
    expect(dock.querySelector('.pane-frame')).toBeNull()
    // And the dock comes AFTER the region in the DOM (pinned at the bottom).
    const region = document.querySelector('.cockpit-panes')!
    expect(region.compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('the PTT hint owns no line of the pinned dock, and its sentence is on the button', () => {
    // DENSITY (2026-08-04). `.ph-ptt-hint` was `flex-basis: 100%` — a whole line of its own
    // inside the PINNED dock, so it cost its ~19px at EVERY window size and no scroller could
    // ever take it back. Its first clause ("Hold the button or the Space bar") is what the
    // button's own title already said, twice on one row.
    //
    // ITS SECOND CLAUSE IS NOT CHROME. "you talk on the rig's mic" is the sentence that stops
    // an operator keying up believing Nexus carries his audio — an on-air failure mode, not
    // 19px. So the row may go only if the sentence lands on the control, and that is what is
    // asserted here rather than the deletion alone.
    renderCockpit()
    expect(
      document.querySelector('.ph-ptt-hint'),
      'the hint still owns a line of the pinned dock',
    ).toBeNull()
    const title = document.querySelector('.ph-ptt')!.getAttribute('title') ?? ''
    expect(title, 'the mic sentence did not survive the row it lived on').toMatch(/rig's mic/i)
    expect(title, 'how to key is no longer stated anywhere on the control').toMatch(/space/i)
  })

  it("⊞ Panels 'removed' still hides exactly the pane it names", () => {
    renderCockpit({ panels: fakePanels(['receiver']) })
    expect(document.querySelector('[data-pane="receiver"]')).toBeNull()
    expect(document.querySelector('[data-pane="transmitter"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="bandActivity"]')).not.toBeNull()
    // PTT is not gated by the menu — it has no id to gate. (That is true of this cockpit's
    // stop-line census, not of "TX chrome": voiceKeyer transmits and is listed.)
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
  })

  // ── THE SCOPE STRIP IS A PANEL NOW (operator, 2026-08-16) ──────────────────────────
  // "add the waterfall in each window as an option to remove in the panels section — leave
  // it ON by default, give me the option to turn it off." Both halves are asserted, and the
  // default half is not a formality: it is the difference between an option and a change.
  it('the scope strip is SHOWN by default — the ⊞ entry is an option, not a new default', () => {
    renderCockpit()
    expect(document.querySelector('.ph-scope-panel'), 'the scope went missing on a stock layout').not.toBeNull()
  })

  it("⊞ 'Scope' unticked takes the strip AND its splitter, and the region takes the height", () => {
    renderCockpit({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.ph-scope-panel'), 'the strip survived its own hide').toBeNull()
    // The seam goes with it. It drags --ph-scope-h, the strip's flex basis, so on its own it is
    // a grab handle for something that is not there — and a shell-level child of its own, which
    // would leave a stranded 8px seam between the header and the region.
    expect(
      document.querySelector('.pane-splitter'),
      'the scope splitter is still in the shell with no scope to size',
    ).toBeNull()
    // The point of the tick: the panes are still there to receive the freed height, which
    // `.cockpit-panes { flex: 1 1 0 }` gives them with no rule change.
    const region = document.querySelector('.cockpit-panes')
    expect(region, 'hiding the scope took the pane region with it').not.toBeNull()
    expect(document.querySelector('[data-pane="log"]')).not.toBeNull()
    // …and the census still holds: one of the stage's child kinds is simply absent.
    const stage = document.querySelector('main.layout.single.phone-cockpit > .cockpit-flat > .cockpit-flat')!
    for (const el of Array.from(stage.children)) {
      expect(
        ['.cockpit-txstrip', '.cockpit-panes'].some((s) => el.matches(s)),
        `unexpected stage child with the scope hidden: <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
  })

  // ── THE STOP LINE, structural half (features/panelState.ts) ────────────────────────
  // The operator must never be unable to stop a transmission. This asserts the SHELL side
  // of that here — the PTT row and the header survive every hide — and it is only half a
  // guard, because this file stubs CockpitHeader down to an empty element, so it can prove
  // the header rendered and nothing about the Stop TX button inside it.
  // components/stop-line.test.tsx renders the REAL header and looks for the actual controls
  // by accessible name, across Phone, CW, RTTY and SSTV. Keep this one anyway: it is the
  // one that catches the PTT row itself, which lives in this cockpit's dock.
  // Run RED by gating the PTT row on `shown('txmeters')` — one line at the real site.
  it('hiding ANY panel in the vocabulary leaves every stop-a-transmission control mounted', () => {
    for (const id of PHONE_PANEL_IDS) {
      renderCockpit({ panels: fakePanels([id]) })
      expect(
        document.querySelector('.cockpit-txdock .ph-ptt'),
        `hiding "${id}" took the PTT button with it`,
      ).not.toBeNull()
      expect(document.querySelector('.cockpit-header'), `hiding "${id}" took the header (Stop TX) with it`).not.toBeNull()
      cleanup()
    }
    // And with the whole vocabulary hidden at once — the state an operator reaches by
    // unticking down the menu — the transmitter can still be shut up.
    renderCockpit({ panels: fakePanels([...PHONE_PANEL_IDS]) })
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
    expect(document.querySelector('.cockpit-header')).not.toBeNull()
  })

  it('the voice keyer is a panel now: docked by default, gone when the operator hides it', () => {
    renderCockpit({ panels: fakePanels() })
    expect(document.querySelector('[data-pane="voiceKeyer"]'), 'keyer not docked by default').not.toBeNull()
    cleanup()
    renderCockpit({ panels: fakePanels(['voiceKeyer']) })
    expect(document.querySelector('[data-pane="voiceKeyer"]'), 'keyer survived its own ⊞ entry').toBeNull()
    // Hiding the keyer is a stop, never a strand: PTT is untouched, and the pane's own
    // unmount aborts the message (PhoneCockpit.keyerHide.test.tsx renders the real one).
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
  })

  it('the leading column is never left empty by the menu (no band of empty panel)', async () => {
    // Reachable the moment the keyer became hideable: unticking everything the leading
    // column can hold used to leave a `minmax(0,1fr)` track holding nothing beside the log
    // — the "empty black box" complaint rebuilt. The tier collapses to 1 instead, and the
    // .cockpit-col count still equals data-cols (useRegionCols' standing invariant).
    //
    // ⚠️ IT TAKES FOUR TICKS NOW, not two. Before the 2026-09-20 rebuild a rig reporting no
    // DSP contributed nothing to this column by itself, so hiding Band Activity and the
    // keyer emptied it. The two chain panes have no capability gate — `shown(id)` is the
    // only thing that can take them away — so the rig's silence no longer empties anything
    // and the operator has to untick them too. Weakening this case to "two ticks and a
    // silent rig" would have left it passing while asserting nothing.
    renderCockpit({
      snap: makeSnap({ nb: null, nr: null, nrLevel: null, agc: null }),
      panels: fakePanels(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter']),
    })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('1')
    expect(region.querySelectorAll(':scope > .cockpit-col').length).toBe(1)
    expect(document.querySelector('[data-pane="log"]')).not.toBeNull()
  })

  it('three columns group band+keyer | aux | log (keyer never leaves the leading column)', async () => {
    renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    const cols = region.querySelectorAll(':scope > .cockpit-col')
    expect(cols.length).toBe(3)
    expect(cols[0].querySelector('[data-pane="bandActivity"]')).not.toBeNull()
    // The keyer shares the leading column at 3-col (NOT design3 §2's "col 2"): a column
    // assignment that changed with the tier would remount the keyer on every flip, and
    // its unmount cleanup aborts an in-flight voice transmission. Fiber stability for a
    // TX-capable pane outranks the grouping aesthetic (fix-round D1, 2026-07-31).
    expect(cols[0].querySelector('[data-pane="voiceKeyer"]')).not.toBeNull()
    expect(cols[0].querySelectorAll('.pane-frame').length).toBe(2)
    expect(cols[1].querySelector('[data-pane="receiver"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="transmitter"]')).not.toBeNull()
    expect(cols[2].querySelector('[data-pane="log"]')).not.toBeNull()
    expect(cols[2].querySelectorAll('.pane-frame').length).toBe(1)
    // Panes are ROLE-typed now, not span-weighted: the strips (band activity's fixed-height
    // strip, the keyer, DSP rows) sit at exactly content height so a chip row can never
    // inflate to half a column of empty panel; the log pane FILLS its column so the recall
    // history gets the room (fix round 2, 2026-07-31).
    // outweighs the strips, so equal rows cannot starve it (design3 §3 fr-share rule).
    expect((document.querySelector('[data-pane="bandActivity"]') as HTMLElement).dataset.fit).toBe('content')
    expect((document.querySelector('[data-pane="voiceKeyer"]') as HTMLElement).dataset.fit).toBe('content')
    expect((document.querySelector('[data-pane="receiver"]') as HTMLElement).dataset.fit).toBe('content')
    const phLog = document.querySelector('[data-pane="log"]') as HTMLElement
    expect(phLog.dataset.fit).toBe('fill')
    expect(phLog.style.flex).toContain('--cockpit-pane-flex')
  })

  // ── TIER FLIPS MUST NOT REMOUNT STATEFUL PANES (fix-round D1, 2026-07-31) ──────────
  // A remount of LogEntry wipes every in-progress QSO field (call/RST/name/… live in
  // plain useState) and a remount of VoiceKeyer fires its unmount cleanup, which ABORTS
  // an in-flight voice transmission (stopVoice) and discards an in-progress recording.
  // The columns are keyed so React reconciles them by identity across the cols ternary;
  // the keyer keeps ONE column (the leading one) at every tier for the same reason.
  // DOM-node identity is the proxy: an unmount destroys the node, so `isSameNode` false
  // ⇒ the fiber died. These tests were run RED against the unkeyed ternary (both nodes
  // were replaced on every 2↔3 flip and on first entry at ≥1700px).
  it('a 2↔3 tier flip keeps the log form and the voice keyer mounted', async () => {
    renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1200)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on 2→3').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'voice keyer remounted on 2→3 (aborts TX)').toBe(true)
    stubWidth(region, 1200)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on 3→2').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'voice keyer remounted on 3→2 (aborts TX)').toBe(true)
  })

  it('first measurement (1→3 in one pass, every section entry at ≥1700px) does not remount them', async () => {
    renderCockpit()
    // The initial commit renders tier 1 (state default); the layout-effect measurement
    // then jumps straight to 3. Before the keyed columns this was a mount→unmount→remount
    // of LogEntry AND VoiceKeyer on every entry to the section — a spurious stop_voice
    // IPC and a doubled getLog fetch.
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on entry').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'voice keyer remounted on entry').toBe(true)
  })

  it('a ⊞ Panels toggle that changes maxCols (no resize at all) does not remount them', async () => {
    // The reviewer's no-resize repro: at ≥1700px, hiding Band Activity flips maxCols 3→2
    // mid-session — the same reconciliation path as a window resize.
    const r = render(
      <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={fakePanels()} />,
    )
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    r.rerender(
      <PhoneCockpit
        snap={makeSnap()}
        theme="dark"
        onWorkSpot={() => {}}
        spots={[]}
        panels={fakePanels(['bandActivity'])}
      />,
    )
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on ⊞ toggle').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'voice keyer remounted on ⊞ toggle').toBe(true)
    // …and the restore back to stock (⊞ Reset layout / Undo) flips maxCols 2→3 again.
    // Vocabulary membership must not have made the keyer's fiber depend on anything but
    // its OWN entry: a menu interaction that merely reorders the region must not abort an
    // over that is on the air while the operator is in the menu.
    r.rerender(
      <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={fakePanels()} />,
    )
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on ⊞ restore').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'voice keyer remounted on ⊞ restore').toBe(true)
  })

  it('TX meters render ABOVE the PTT row in the bottom-anchored dock', () => {
    // The dock is `flex: 0 0 auto; position: sticky; bottom: 0` — bottom-anchored — and
    // TxMeters grows on key-down (one line of idle hint → up to four rows of readings;
    // before it was pinned, from nothing at all). Below the PTT row that growth pushes the
    // dock UP under the held pointer: the button shifts, `onPointerLeave` fires, TX drops
    // mid-over. CW names this hazard and puts the meters first; Phone must match.
    renderCockpit({ snap: makeSnap({ transmitting: true, txSwr: 1.2 }) })
    const meters = document.querySelector('.cockpit-txdock .ph-txmeters')
    const ptt = document.querySelector('.cockpit-txdock .ph-ptt-row')
    expect(meters, 'no TX meters in the dock while keyed').not.toBeNull()
    expect(ptt, 'no PTT row in the dock').not.toBeNull()
    expect(
      meters!.compareDocumentPosition(ptt!) & Node.DOCUMENT_POSITION_FOLLOWING,
      'TxMeters render below the PTT row: mounting on key-down shifts the button under the operator\'s pointer',
    ).toBeTruthy()
  })

  it('maxCols caps the tier at 2 when a third column would sit empty', async () => {
    // No aux pane renders → even an ultrawide region stays 2-col: a 3-track template with
    // an empty middle is the "band of empty black" rebuilt.
    //
    // ⚠️ THE AUX COLUMN IS EMPTIED BY THE MENU NOW, not by a silent rig. Its occupants are
    // the rig-scope pane (which needs a native panadapter streaming, and the stubbed
    // PhoneScope never reports one) and the two chain panes, which always render until they
    // are unticked. The old fixture — a rig reporting no DSP — leaves both chain panes up,
    // so it would now assert 3 and prove nothing about the cap.
    renderCockpit({
      snap: makeSnap({ nb: null, nr: null, nrLevel: null, agc: null }),
      panels: fakePanels(['receiver', 'transmitter']),
    })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(region.querySelectorAll(':scope > .cockpit-col').length).toBe(2)
    cleanup()

    // Band Activity absent (no onWorkSpot wire) → the leading 3-col track would be empty,
    // so the tier is likewise capped at 2 with aux panes present. (No `panels` prop at all
    // here, which is the remote observer's shape: every pane shows and the ⊞ menu hides.)
    render(<PhoneCockpit snap={makeSnap()} theme="dark" spots={[]} />)
    const region2 = document.querySelector('.cockpit-panes')!
    stubWidth(region2, 1800)
    act(() => fire!())
    await frame()
    expect(region2.getAttribute('data-cols')).toBe('2')
  })
})

// ── SPOTS AND NEEDED (#345): two FEEDS where the empty real estate is ────────────────────
//
// They are fill panes (a list stretches; a strip does not) and they go below the strips, where
// the surplus the tester reported is: both at the foot of the LEADING column at tiers 1 and 2,
// and at tier 3 Spots there and Needed at the foot of the middle column (measured in Chrome: in
// one column at 1920×1080 Needed sat below the fold while the middle column stood empty). The
// log and the keyer keep their columns and their places in them, so the keyed-column rule above
// is untouched — and it is re-run here WITH the feeds shown, because a new child in the keyer's
// own column is exactly how a sibling's position (and so its fiber) could move.
describe('PhoneCockpit Spots and Needed feeds', () => {
  const wiring = {
    spotsBoard: { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} },
    neededBoard: { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} },
  }
  const withFeeds = (panels = fakePanels()) => (
    <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} {...wiring} />
  )
  const framesIn = (col: Element) =>
    [...col.querySelectorAll(':scope > .pane-frame')].map((f) => (f as HTMLElement).dataset.pane)
  async function tier(region: Element, width: number) {
    stubWidth(region, width)
    act(() => fire!())
    await frame()
  }

  it('close the leading column at tiers 1 and 2; at tier 3 Spots leads and Needed takes the middle', async () => {
    render(withFeeds())
    const region = document.querySelector('.cockpit-panes')!
    for (const [width, cols] of [[0, '1'], [1200, '2']] as const) {
      if (width) await tier(region, width)
      expect(region.getAttribute('data-cols')).toBe(cols)
      const lead = region.querySelector(':scope > .cockpit-col')!
      expect(framesIn(lead).slice(0, 2), `tier ${cols}: band + keyer no longer lead`).toEqual(['bandActivity', 'voiceKeyer'])
      expect(framesIn(lead).slice(-2), `tier ${cols}: the feeds left the leading column`).toEqual(['spots', 'needed'])
      const log = region.querySelector(':scope > .cockpit-col:last-child')!
      expect(framesIn(log), `tier ${cols}: the log column holds more than the log`).toEqual(['log'])
    }
    await tier(region, 1800)
    expect(region.getAttribute('data-cols')).toBe('3')
    const cols3 = region.querySelectorAll(':scope > .cockpit-col')
    expect(framesIn(cols3[0])).toEqual(['bandActivity', 'voiceKeyer', 'spots'])
    expect(framesIn(cols3[1])).toEqual(['receiver', 'transmitter', 'needed'])
    expect(framesIn(cols3[2])).toEqual(['log'])
    for (const id of ['spots', 'needed']) {
      const f = document.querySelector(`[data-pane="${id}"]`) as HTMLElement
      expect(f.dataset.fit, `${id} is a content strip — a list must be able to use the surplus`).toBe('fill')
    }
  })

  // Needed is not on these lists: at tier 3 it changes column, as the strips do, and remounts —
  // its filters are stored, so what it loses is its sort order and scroll. Spots does not move.
  it('a 2↔3 flip with both feeds shown keeps the log form, the keyer AND Spots mounted', async () => {
    render(withFeeds())
    const region = document.querySelector('.cockpit-panes')!
    await tier(region, 1200)
    expect(region.getAttribute('data-cols')).toBe('2')
    const nodes = ['log-stub', 'vk-stub', 'spots-stub'].map((id) => [id, document.querySelector(`[data-testid="${id}"]`)!] as const)
    for (const width of [1800, 1200, 1800]) {
      await tier(region, width)
      for (const [id, n0] of nodes) {
        expect(document.querySelector(`[data-testid="${id}"]`)!.isSameNode(n0), `${id} remounted on a flip to ${width}px`).toBe(true)
      }
    }
  })

  it('first measurement (1→3 in one pass) with the feeds shown remounts neither the log, the keyer nor Spots', async () => {
    render(withFeeds())
    const nodes = ['log-stub', 'vk-stub', 'spots-stub'].map((id) => [id, document.querySelector(`[data-testid="${id}"]`)!] as const)
    await tier(document.querySelector('.cockpit-panes')!, 1800)
    for (const [id, n0] of nodes) {
      expect(document.querySelector(`[data-testid="${id}"]`)!.isSameNode(n0), `${id} remounted on entry`).toBe(true)
    }
  })

  it('ticking a feed on or off never remounts the log form or the keyer', async () => {
    const r = render(withFeeds(fakePanels(['spots', 'needed'])))
    const region = document.querySelector('.cockpit-panes')!
    await tier(region, 1800)
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    for (const removed of [['needed'], [], ['spots'], ['spots', 'needed']] as PhonePanelId[][]) {
      r.rerender(withFeeds(fakePanels(removed)))
      await frame()
      expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), `log remounted at {${removed}}`).toBe(true)
      expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), `keyer remounted at {${removed}}`).toBe(true)
    }
  })

  it('the feeds alone hold their tracks, and no track is ever left empty', async () => {
    // Everything else unticked: at tier 3 Spots holds the leading track and Needed the middle one,
    // so an ultrawide still gets three — none of them empty (the "empty black" rule counts them).
    const r = render(withFeeds(fakePanels(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter'])))
    const region = document.querySelector('.cockpit-panes')!
    await tier(region, 1800)
    expect(region.getAttribute('data-cols')).toBe('3')
    let cols = region.querySelectorAll(':scope > .cockpit-col')
    expect([...cols].map(framesIn)).toEqual([['spots'], ['needed'], ['log']])
    // Spots alone as well: nothing for a middle track, so an ultrawide stays at two.
    r.rerender(withFeeds(fakePanels(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter', 'needed'])))
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    cols = region.querySelectorAll(':scope > .cockpit-col')
    expect([...cols].map(framesIn)).toEqual([['spots'], ['log']])
    // …and Needed alone, with nothing in the leading track at tier 3: two, Needed leading.
    r.rerender(withFeeds(fakePanels(['bandActivity', 'voiceKeyer', 'receiver', 'transmitter', 'spots'])))
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    cols = region.querySelectorAll(':scope > .cockpit-col')
    expect([...cols].map(framesIn)).toEqual([['needed'], ['log']])
  })

  it('without the board wiring (no host to feed them) neither pane renders, ticked or not', () => {
    render(<PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={fakePanels()} />)
    expect(document.querySelector('[data-pane="spots"]')).toBeNull()
    expect(document.querySelector('[data-pane="needed"]')).toBeNull()
  })
})

// ── the TX meters are a teaching instrument, so they cannot live only mid-over ─────────
//
// SWR, ALC, Po and COMP used to render ONLY while keyed: the panel appeared on key-down and
// vanished on release. For a voice operator that is the wrong half of the QSO — the meter is
// how you learn your own drive, and a reading you can never look at without also holding the
// mic key teaches nothing. Operate has had the answer since the anti-bounce ruling: `pinned`
// retains the last live readings between overs and shows a fixed-height hint before the first
// one. Phone and CW now pass the same prop; CwCockpit.structure.test.tsx is this test's twin.
describe('PhoneCockpit TX meters are pinned, not flashed', () => {
  const meters = () => document.querySelector('.cockpit-txdock .ph-txmeters')
  const at = (over: Record<string, unknown>) => (
    <PhoneCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />
  )

  it('keeps the last over on screen after the key is released', () => {
    // BEFORE THE FIRST OVER — the panel is on screen saying when it reads. It used to render
    // nothing at all here, which is indistinguishable from a panel Nexus never built.
    const r = render(at({}))
    expect(meters(), 'no TX meters panel while receiving').not.toBeNull()
    expect(meters()!.textContent).toContain('readings appear on transmit')

    // KEYED — the live reading, undimmed.
    r.rerender(at({ transmitting: true, txSwr: 2.5, swrScaleVerified: true }))
    expect(meters()!.textContent).toContain('2.5:1')
    expect(meters()!.classList.contains('idle')).toBe(false)

    // ⭐ UNKEYED, AND THE RIG HAS STOPPED REPORTING — which is what makes this assertion able
    // to differ. The snapshot carries no SWR now, so a panel that merely re-rendered what it
    // was handed would show the hint again (or, before this change, nothing); 2.5:1 can only
    // be on screen because the last live reading was RETAINED, and `idle` is what says the
    // operator is looking at a memory rather than a live needle.
    r.rerender(at({ txSwr: null }))
    expect(meters()!.textContent).toContain('2.5:1')
    expect(meters()!.classList.contains('idle')).toBe(true)
    expect(meters()!.textContent).not.toContain('readings appear on transmit')
  })
})

// ── THE SCOPE DIVIDER (layout L1, PaneSeam) ─────────────────────────────────────────────────
// The scope's height divider must be reachable from the keyboard and say where it stands. jsdom
// lays nothing out, so the shell gets a size before the cockpit mounts; with no --vh-eff and a
// 16 px font the sheet's clamps resolve to 8em = 128 px and 0.45 · 768 = 345.6 px.
describe('the scope divider answers the keyboard (PaneSeam)', () => {
  function layOut(boxes: Record<string, { top?: number; left?: number; width?: number; height?: number }>) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      for (const [sel, b] of Object.entries(boxes)) {
        if (!this.matches(sel)) continue
        const { top = 0, left = 0, width = 800, height = 0 } = b
        return { top, left, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) } as DOMRect
      }
      return real.call(this)
    })
  }
  // The variable lives on the scope's PARENT — the stage — and inherits to it whatever box it is laid
  // out in; the box measured is the stage's nearest one: the SHELL while the stage is the box-less
  // `.cockpit-flat` (no side shown), the stage itself once the side shows (below).
  const stage = () => document.querySelector<HTMLElement>('.ph-scope-panel')!.parentElement!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  // jsdom loads no stylesheet: the wrappers' `display` comes from the REAL sheet's rule, read out of
  // cockpit-panes.css, so the divider walks past exactly the box-less wrapper the app renders.
  let sheet: HTMLStyleElement | null = null
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--vh-eff')
    document.documentElement.style.removeProperty('--ui-zoom')
    const css = readFileSync(resolve(process.cwd(), 'src/cockpit-panes.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    sheet = document.createElement('style')
    sheet.textContent = [...css.matchAll(/(^|})\s*(\.cockpit-(flat|stage|leftrow)\s*\{[^}]*\})/g)].map((m) => m[2]).join('\n')
    document.head.appendChild(sheet)
  })
  afterEach(() => {
    vi.restoreAllMocks()
    sheet?.remove()
  })

  it('the wrappers the divider walks past are the sheet’s own (control: the rule really loaded)', () => {
    renderCockpit()
    expect(getComputedStyle(stage()).display).toBe('contents')
    expect(stage().classList.contains('cockpit-flat')).toBe(true)
  })

  it('focusable, announces its height in CSS px, and steps, jumps and resets', () => {
    layOut({ 'main.phone-cockpit': { height: 1000 } })
    renderCockpit()
    const sep = screen.getByRole('separator', { name: 'scope height' })
    expect(sep.tabIndex, 'a divider only a mouse can reach').toBe(0)
    expect(aria(sep)).toEqual(['220', '128', '346'])
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(stage().style.getPropertyValue('--ph-scope-h')).toBe(`${(236 / 1000) * 100}%`)
    expect(localStorage.getItem('nexus.split.phone.scope')).toBe(String((236 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'ArrowDown', shiftKey: true })
    expect(sep.getAttribute('aria-valuenow')).toBe('300')
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('346')
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow')).toBe('128')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(stage().style.getPropertyValue('--ph-scope-h')).toBe('22%')
    expect(localStorage.getItem('nexus.split.phone.scope')).toBe('22')
  })

  it('a height stored by an earlier build is restored, clamped against this window, and kept', () => {
    localStorage.setItem('nexus.split.phone.scope', '30')
    layOut({ 'main.phone-cockpit': { height: 1000 } })
    const first = renderCockpit()
    expect(stage().style.getPropertyValue('--ph-scope-h')).toBe('30%')
    first.unmount()
    localStorage.setItem('nexus.split.phone.scope', '75')
    renderCockpit()
    expect(screen.getByRole('separator', { name: 'scope height' }).getAttribute('aria-valuenow')).toBe('346')
    expect(localStorage.getItem('nexus.split.phone.scope'), 'the clamp is apply-side only').toBe('75')
  })

  it('with the left side shown the scope keeps its HEIGHT: a share of the shell, painted as a share of the stage', () => {
    windowWidth(1600)
    layOut({ 'main.phone-cockpit': { height: 1000 }, '.cockpit-stage': { height: 800 } })
    renderCockpit({ panels: sidePanels(['bandActivity']) })
    expect(stage().classList.contains('cockpit-stage'), 'the side did not show').toBe(true)
    expect(getComputedStyle(stage()).display).toBe('flex')
    // Beside the side the scope's intrinsic size is its floor (styles.css `.ph-scope-panel--beside`).
    expect(document.querySelector('.ph-scope-panel')!.classList.contains('ph-scope-panel--beside')).toBe(true)
    const sep = screen.getByRole('separator', { name: 'scope height' })
    // 22 % of the SHELL's 1000 — the 220 px it stands at with no side — not 22 % of the stage's 800:
    // showing the side narrows the scope and leaves its height, and the strip under it, alone.
    expect(aria(sep)).toEqual(['220', '128', '346'])
    const painted = () => parseFloat(stage().style.getPropertyValue('--ph-scope-h'))
    expect(painted()).toBeCloseTo((220 / 800) * 100, 6)
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(painted()).toBeCloseTo((236 / 800) * 100, 6)
    // Stored as a share of the shell, as it always was: the same value means the same height.
    expect(Number(localStorage.getItem('nexus.split.phone.scope'))).toBeCloseTo((236 / 1000) * 100, 6)
  })
})

// ── THE COLUMN DIVIDERS (layout L2) ──────────────────────────────────────────────────────────
// What the divider itself does (keys, values, drag, clamp, Reset/Undo) is RegionColumnSeams.test;
// this is Phone's wiring of it: which divider sits over which of ITS columns at each tier, that
// the operator's widths ride its region, and that a divider's commit remounts nothing.
describe('PhoneCockpit column dividers', () => {
  // Every live observer hears a resize, as in a browser: the region's and the width divider's.
  let live: Set<() => void>
  const resize = () => act(() => [...live].forEach((cb) => cb()))
  beforeEach(() => {
    live = new Set()
    localStorage.clear()
    globalThis.ResizeObserver = class {
      cb: () => void
      constructor(cb: () => void) {
        this.cb = cb
        live.add(cb)
      }
      observe() {}
      disconnect() {
        live.delete(this.cb)
      }
      unobserve() {}
    } as unknown as typeof ResizeObserver
  })
  function Live() {
    const panels = usePanelLayout(PHONE_PANELS)
    return <PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />
  }
  const dividers = (region: Element) =>
    [...region.querySelectorAll(':scope > [role="separator"]')].map((s) => [
      s.getAttribute('aria-label'),
      [...s.classList].filter((c) => c.startsWith('cockpit-colseam-')).join(' '),
    ])
  async function tier(region: Element, width: number) {
    stubWidth(region, width)
    resize()
    await frame()
  }

  it('none in the stacking tier; at two columns the log width; at three the split between the feed columns as well', async () => {
    render(<Live />)
    const region = document.querySelector('.cockpit-panes')!
    expect(region.getAttribute('data-cols')).toBe('1')
    expect(dividers(region), 'a stack cannot be divided sideways').toEqual([])
    await tier(region, 1200)
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(dividers(region)).toEqual([['log column width', 'cockpit-colseam-2']])
    await tier(region, 1800)
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(dividers(region)).toEqual([
      ['Band activity column / Receiver column', 'cockpit-colseam-2'],
      ['log column width', 'cockpit-colseam-3'],
    ])
    // The region's own children, after every column: the columns keep their order and count.
    const kinds = [...region.children].map((c) => (c.getAttribute('role') === 'separator' ? 'sep' : 'col'))
    expect(kinds).toEqual(['col', 'col', 'col', 'sep', 'sep'])
    await tier(region, 900)
    expect(dividers(region)).toEqual([])
  })

  it('the widths stored in the Phone record ride the region; a record from before them adds none', () => {
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 2, state: {}, share: {}, cols: { a: 1.3, b: 0.7, log: 560 } }))
    const { unmount } = render(<Live />)
    const region = document.querySelector<HTMLElement>('.cockpit-panes')!
    expect(region.style.getPropertyValue('--cockpit-col-a')).toBe('1.3fr')
    expect(region.style.getPropertyValue('--cockpit-col-b')).toBe('0.7fr')
    expect(region.style.getPropertyValue('--cockpit-col-log')).toBe('min(560px, 50%)')
    unmount()
    localStorage.setItem(panelStorageKey('phone'), JSON.stringify({ v: 1, state: {}, share: {} }))
    render(<Live />)
    expect(document.querySelector<HTMLElement>('.cockpit-panes')!.getAttribute('style') ?? '').not.toMatch(/--cockpit-col/)
  })

  it('moving the log divider remounts neither the log form nor the voice keyer', async () => {
    render(<Live />)
    const region = document.querySelector('.cockpit-panes')!
    await tier(region, 1200)
    const logCol = [...region.querySelectorAll<HTMLElement>(':scope > .cockpit-col')].slice(-1)[0]
    logCol.getBoundingClientRect = () => ({ left: 712, right: 1200, width: 488, top: 0, bottom: 400, height: 400, x: 712, y: 0, toJSON: () => ({}) }) as DOMRect
    resize()
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const vk0 = document.querySelector('[data-testid="vk-stub"]')!
    const sep = screen.getByRole('separator', { name: 'log column width' })
    fireEvent.keyDown(sep, { key: 'ArrowLeft' })
    expect(JSON.parse(localStorage.getItem(panelStorageKey('phone'))!).cols).toEqual({ log: 504 })
    expect((region as HTMLElement).style.getPropertyValue('--cockpit-col-log')).toBe('min(504px, 50%)')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'the log form remounted').toBe(true)
    expect(document.querySelector('[data-testid="vk-stub"]')!.isSameNode(vk0), 'the voice keyer remounted (aborts TX)').toBe(true)
  })
})

// ── THE STOP LINE'S STRIP, past the wrappers (2026-10-03) ─────────────────────────────────────────
// The TX strip parks above the sticky dock by its `--cockpit-txstrip-bottom`, the dock's height,
// which it measures by finding the dock among its SHELL's children. Since the left side, the strip's
// parent is the stage, not the shell: a lookup by parent finds no dock, writes 0 and parks Stop TX
// UNDER the PTT row at a large pin (measured 2026-10-01). jsdom loads no sheet, so the two `sticky`s are
// injected and the dock's height stubbed; what is computed is that the strip finds the dock.
describe('the TX strip still clears the sticky dock from inside the stage', () => {
  let sheet: HTMLStyleElement | null = null
  beforeEach(() => {
    sheet = document.createElement('style')
    sheet.textContent = '.cockpit-txdock { position: sticky } .cockpit-txstrip { position: sticky }'
    document.head.appendChild(sheet)
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.classList.contains('cockpit-txdock') ? 117 : 0
    })
  })
  afterEach(() => {
    vi.restoreAllMocks()
    sheet?.remove()
  })

  for (const [state, side] of [['no side', null], ['left side shown', 1600]] as const) {
    it(`${state}: the strip's bottom offset is the dock's height, and its height is on the shell`, () => {
      windowWidth(side)
      renderCockpit({ panels: side ? sidePanels(['bandActivity']) : fakePanels() })
      expect(document.querySelector('.cockpit-left') != null, 'the side state is not the one this case is about').toBe(side != null)
      const strip = document.querySelector<HTMLElement>('.cockpit-txstrip')!
      expect(strip.parentElement!.matches('main'), 'the strip is a shell child again — this case tests nothing').toBe(false)
      expect(strip.style.getPropertyValue('--cockpit-txstrip-bottom'), 'Stop TX would park under the PTT row').toBe('117px')
      const shell = document.querySelector<HTMLElement>('main.phone-cockpit')!
      expect(shell.style.getPropertyValue('--cockpit-txstrip-h'), 'the scroll padding left the shell').not.toBe('')
    })
  }
})
