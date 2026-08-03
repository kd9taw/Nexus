// @vitest-environment jsdom
//
// PHONE COCKPIT SHELL STRUCTURE (2026-07-30 layout assessment, design3 §3/§5).
//
// The shell contract has exactly four child kinds: header chrome, the scope, ONE pane
// region (.cockpit-panes, tier-stamped by useRegionCols) and the pinned TX dock. These
// tests pin the parts of that contract that live in TSX, where no CSS test can see them:
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
import { render, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS } from '../features/panelState'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

vi.mock('../api', () => ({
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
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** The observed element's callback, so a test can fire a resize the way the browser
 *  does (the useRegionCols.test.tsx harness). */
let fire: (() => void) | null = null
beforeEach(() => {
  fire = null
  globalThis.ResizeObserver = class {
    constructor(cb: () => void) {
      fire = cb
    }
    observe() {}
    disconnect() {}
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
    reset: () => {},
  }
}

const renderCockpit = (props: Partial<Parameters<typeof PhoneCockpit>[0]> = {}) =>
  render(<PhoneCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)

describe('PhoneCockpit pane-grid shell', () => {
  it('the shell holds no child kinds beyond the contract (design3 §5 rule 1)', () => {
    // The recurrence-proof leans on this: "making X unreachable would require adding a
    // shell-level sibling, which fails contract test 1" — so the test has to exist. The
    // sanctioned kinds are header chrome, the scope (+ its Splitter grip), ONE pane
    // region, ONE TX dock, and modal chrome (SpotDialog portals in as .logconfirm-backdrop
    // when open; mocked null here). Anything else is a new shell-level sibling and must
    // update this census — deliberately, with a name — not slip in.
    renderCockpit({ snap: makeSnap({ transmitting: true, txSwr: 1.2 }) })
    const shell = document.querySelector('main.layout.single.phone-cockpit')!
    const ALLOWED = [
      '.cockpit-header',
      '.ph-scope-panel',
      '.pane-splitter',
      '.cockpit-panes',
      '.cockpit-txdock',
      '.logconfirm-backdrop',
    ]
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
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
    for (const id of ['bandActivity', 'voiceKeyer', 'log', 'dsp', 'dspLevels']) {
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

  it('PTT lives in the pinned TX dock — never inside a pane or the region', () => {
    renderCockpit()
    const ptt = document.querySelector('.ph-ptt')
    expect(ptt, 'no PTT button').not.toBeNull()
    expect(ptt!.closest('.cockpit-txdock'), 'PTT is not in the TX dock').not.toBeNull()
    expect(ptt!.closest('.pane-frame'), 'PTT is inside a pane frame — a pane can scroll it away').toBeNull()
    expect(ptt!.closest('.cockpit-panes'), 'PTT is inside the pane region').toBeNull()
    // The dock holds no pane frames at all — TX chrome has no id in the pane vocabulary.
    const dock = document.querySelector('.cockpit-txdock')!
    expect(dock.querySelector('.pane-frame')).toBeNull()
    // And the dock comes AFTER the region in the DOM (pinned at the bottom).
    const region = document.querySelector('.cockpit-panes')!
    expect(region.compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it("⊞ Panels 'removed' still hides exactly the pane it names", () => {
    renderCockpit({ panels: fakePanels(['dsp']) })
    expect(document.querySelector('[data-pane="dsp"]')).toBeNull()
    expect(document.querySelector('[data-pane="dspLevels"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="bandActivity"]')).not.toBeNull()
    // TX chrome is not gated by the menu — it has no id to gate.
    expect(document.querySelector('.cockpit-txdock .ph-ptt')).not.toBeNull()
  })

  // ── THE STOP LINE (operator ruling, 2026-08-03) ────────────────────────────────────
  // A pane that can only START a transmission may be hidden; anything that can STOP one
  // may never be. This is the guard that COMPUTES it: it drives every id in the real
  // vocabulary through the real hide path and looks at what is left standing. A name list
  // (features/panelState.test.ts) cannot see a vocabulary id wired to the PTT row; this
  // can. Run RED by gating the PTT row on `shown('txmeters')` — one line at the real site.
  it('hiding ANY panel in the vocabulary leaves every stop-a-transmission control mounted', () => {
    for (const id of PHONE_PANEL_IDS) {
      renderCockpit({ panels: fakePanels([id]) })
      expect(
        document.querySelector('.cockpit-txdock .ph-ptt'),
        `hiding "${id}" took the PTT button with it`,
      ).not.toBeNull()
      // Stop TX lives in the CockpitHeader (stubbed here down to its element): the
      // assertion is that no panel id gates the header's render.
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
    // Reachable the moment the keyer became hideable: a rig that reports no DSP and no
    // native scope has nothing in the leading column but Band Activity and the keyer, so
    // unticking both used to leave a `minmax(0,1fr)` track holding nothing beside the log
    // — the "empty black box" complaint rebuilt. The tier collapses to 1 instead, and the
    // .cockpit-col count still equals data-cols (useRegionCols' standing invariant).
    renderCockpit({
      snap: makeSnap({ nb: null, nr: null, nrLevel: null, agc: null }),
      panels: fakePanels(['bandActivity', 'voiceKeyer']),
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
    expect(cols[1].querySelector('[data-pane="dsp"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="dspLevels"]')).not.toBeNull()
    expect(cols[2].querySelector('[data-pane="log"]')).not.toBeNull()
    expect(cols[2].querySelectorAll('.pane-frame').length).toBe(1)
    // Panes are ROLE-typed now, not span-weighted: the strips (band activity's fixed-height
    // strip, the keyer, DSP rows) sit at exactly content height so a chip row can never
    // inflate to half a column of empty panel; the log pane FILLS its column so the recall
    // history gets the room (fix round 2, 2026-07-31).
    // outweighs the strips, so equal rows cannot starve it (design3 §3 fr-share rule).
    expect((document.querySelector('[data-pane="bandActivity"]') as HTMLElement).dataset.fit).toBe('content')
    expect((document.querySelector('[data-pane="voiceKeyer"]') as HTMLElement).dataset.fit).toBe('content')
    expect((document.querySelector('[data-pane="dsp"]') as HTMLElement).dataset.fit).toBe('content')
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
    // TxMeters mounts only while transmitting. Below the PTT row it grows the dock UP
    // under the held pointer: the button shifts, `onPointerLeave` fires, TX drops
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
    // No aux pane can render (rig reports no DSP capability and no scope feed) → even an
    // ultrawide region stays 2-col: a 3-track template with an empty middle is the "band
    // of empty black" rebuilt.
    renderCockpit({ snap: makeSnap({ nb: null, nr: null, nrLevel: null, agc: null }) })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(region.querySelectorAll(':scope > .cockpit-col').length).toBe(2)
    cleanup()

    // Band Activity absent (no onWorkSpot wire) → the leading 3-col track would be empty,
    // so the tier is likewise capped at 2 with aux panes present.
    render(<PhoneCockpit snap={makeSnap()} theme="dark" spots={[]} />)
    const region2 = document.querySelector('.cockpit-panes')!
    stubWidth(region2, 1800)
    act(() => fire!())
    await frame()
    expect(region2.getAttribute('data-cols')).toBe('2')
  })
})
