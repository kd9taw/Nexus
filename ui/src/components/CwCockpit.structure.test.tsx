// @vitest-environment jsdom
//
// CW COCKPIT SHELL STRUCTURE (2026-07-30 layout assessment, design3 §3/§5).
//
// The twin of PhoneCockpit.structure.test.tsx, pinning the same contract on the cockpit
// that has the MOST content blocks (eight ids in the ⊞ vocabulary): header chrome, the
// scope, ONE pane region and the pinned TX dock.
//
// The CW-specific clauses:
//   - DECODE is the wide pane — it leads the region, and at three columns it gets the
//     leading (1.6fr) track, exactly where Phone puts Band Activity.
//   - The F-key MACROS and the type-ahead send bar are TX chrome, so they render in
//     .cockpit-txdock, never in a pane. A macro is a one-click transmit; a pane scrolls.
//     (design3 §4 proposed "macros become a pane" and then, three words later, moved the
//     send bar to the dock "because key/abort must obey the same reachability rule as
//     PTT" — the two halves contradict each other and the reachability rule wins.)
//   - TX meters render in the dock beside the send controls (they self-null between
//     overs, so a frame around them would be a titled empty box every time you stop
//     keying — the "empty black box" complaint rebuilt).
//
// Heavy children are stubbed: this asserts the SHELL's structure, not pane behaviour.
// jsdom has no layout, so widths are stubbed the way useRegionCols.test.tsx does.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot } from '../types'
import type { CwPanelId, PanelLayoutApi } from '../features/panelState'

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

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
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
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** The observed element's callback, so a test can fire a resize the way the browser does. */
let fire: (() => void) | null = null
beforeEach(() => {
  fire = null
  decodeState.sent = []
  decodeState.keyerError = null
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

/** Snapshot of a CAT rig that reports NB/NR + NR-level/AGC, so the DSP and RX-DSP-levels
 *  aux panes render. (scopeCtl needs a live native scope feed, which the stubbed PhoneScope
 *  never reports — deliberately out of scope here, exactly as in the Phone twin.) */
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

function fakePanels(removed: CwPanelId[] = []): PanelLayoutApi<CwPanelId> {
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

async function renderCockpit(props: Partial<Parameters<typeof CwCockpit>[0]> = {}) {
  const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} {...props} />)
  // Let the mount-time getSettings / cwDecode / previewCw promises land.
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

describe('CwCockpit pane-grid shell', () => {
  it('the shell holds no child kinds beyond the contract (design3 §5 rule 1)', async () => {
    // Same census as the Phone twin. CW's extra sanctioned kind is the keyer-error
    // banner (.cw-keyer-warn) — alert chrome, in-flow above the scope. SpotDialog
    // (.logconfirm-backdrop when open) is mocked null here.
    decodeState.keyerError = 'no keying port'
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.cw-cockpit')!
    const ALLOWED = [
      '.cockpit-header',
      '.cw-keyer-warn',
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
    expect(document.querySelector('.cw-keyer-warn'), 'warn banner did not render — census untested').not.toBeNull()
  })

  it('renders exactly one .cockpit-panes region, tier-stamped by useRegionCols', async () => {
    await renderCockpit()
    const regions = document.querySelectorAll('.cockpit-panes')
    expect(regions.length).toBe(1)
    // jsdom width 0 → the hook keeps its initial tier 1, stamped before first paint.
    expect(regions[0].getAttribute('data-cols')).toBe('1')
    // Tier 1 renders the two-column grouping (it stacks — the region is the scroller).
    expect(regions[0].querySelectorAll(':scope > .cockpit-col').length).toBe(2)
  })

  it('every operator-content block renders through a CockpitPaneFrame inside the region', async () => {
    decodeState.sent = ['CQ CQ DE KD9TAW K']
    await renderCockpit()
    for (const id of ['decode', 'sent', 'bandActivity', 'copilot', 'dsp', 'rxdsp', 'log']) {
      const pane = document.querySelector(`[data-pane="${id}"]`)
      expect(pane, `pane "${id}" missing`).not.toBeNull()
      expect(pane!.classList.contains('pane-frame'), `"${id}" is not a .pane-frame`).toBe(true)
      expect(pane!.closest('.cockpit-panes'), `"${id}" renders outside the region`).not.toBeNull()
    }
    // The frames actually contain their content (not empty shells beside it).
    expect(document.querySelector('[data-pane="decode"] .cw-decode-text')).not.toBeNull()
    expect(document.querySelector('[data-pane="log"] [data-testid="log-stub"]')).not.toBeNull()
    expect(
      document.querySelector('[data-pane="bandActivity"] [data-testid="bandstrip-stub"]'),
    ).not.toBeNull()
  })

  it('macros, the send bar and the TX meters live in the pinned dock — never in a pane', async () => {
    await renderCockpit({ snap: makeSnap({ transmitting: true, swr: 1.2 }) })
    const dock = document.querySelector('.cockpit-txdock')
    expect(dock, 'no .cockpit-txdock').not.toBeNull()
    for (const sel of ['.cw-macros', '.cw-send', '.cw-type', '.cw-send-btn', '.cw-macro']) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.cockpit-txdock'), `${sel} is not in the TX dock`).not.toBeNull()
      expect(
        el!.closest('.pane-frame'),
        `${sel} is inside a pane frame — a pane scrolls, transmit controls must not`,
      ).toBeNull()
      expect(el!.closest('.cockpit-panes'), `${sel} is inside the pane region`).toBeNull()
    }
    // The dock holds no pane frames at all — TX chrome has no id in the pane vocabulary.
    expect(dock!.querySelector('.pane-frame')).toBeNull()
    // And the dock comes AFTER the region in the DOM (pinned at the bottom).
    const region = document.querySelector('.cockpit-panes')!
    expect(region.compareDocumentPosition(dock!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it("⊞ Panels 'removed' still hides exactly the pane it names", async () => {
    await renderCockpit({ panels: fakePanels(['decode']) })
    expect(document.querySelector('[data-pane="decode"]')).toBeNull()
    expect(document.querySelector('[data-pane="copilot"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="log"]')).not.toBeNull()
    // TX chrome is not gated by the menu — it has no id to gate.
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-macro')).not.toBeNull()
  })

  it('three columns group decode+sent | aux | log (decode leads the wide track)', async () => {
    decodeState.sent = ['CQ CQ DE KD9TAW K']
    await renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    const cols = region.querySelectorAll(':scope > .cockpit-col')
    expect(cols.length).toBe(3)
    expect(cols[0].querySelector('[data-pane="decode"]')).not.toBeNull()
    expect(cols[0].querySelector('[data-pane="sent"]')).not.toBeNull()
    expect(cols[0].querySelectorAll('.pane-frame').length).toBe(2)
    expect(cols[1].querySelector('[data-pane="dsp"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="rxdsp"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="bandActivity"]')).not.toBeNull()
    expect(cols[1].querySelector('[data-pane="copilot"]')).not.toBeNull()
    expect(cols[2].querySelector('[data-pane="log"]')).not.toBeNull()
    expect(cols[2].querySelectorAll('.pane-frame').length).toBe(1)
    // Prominence is a row-weight (grid-row span): DECODE outweighs the control strips,
    // so equal minmax(0,1fr) rows cannot starve the transcript the operator reads.
    expect((document.querySelector('[data-pane="decode"]') as HTMLElement).style.gridRow).toBe('span 3')
    expect((document.querySelector('[data-pane="sent"]') as HTMLElement).style.gridRow).toBe('span 2')
  })

  // ── TIER FLIPS MUST NOT REMOUNT THE LOG FORM (fix-round D1, 2026-07-31) ────────────
  // LogEntry keeps every in-progress QSO field in plain useState; a remount wipes them
  // mid-QSO, and the click-to-work prefill cannot come back (pendingWork was consumed).
  // The columns are keyed so React reconciles them by identity across the cols ternary.
  // Node identity is the proxy for fiber survival — see the Phone twin. Run RED against
  // the unkeyed ternary before the fix.
  it('a 2↔3 tier flip keeps the log form (and the transcripts) mounted', async () => {
    decodeState.sent = ['CQ CQ DE KD9TAW K']
    await renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1200)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    const log0 = document.querySelector('[data-testid="log-stub"]')!
    const decode0 = document.querySelector('[data-pane="decode"]')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on 2→3').toBe(true)
    expect(document.querySelector('[data-pane="decode"]')!.isSameNode(decode0), 'decode transcript remounted on 2→3').toBe(true)
    stubWidth(region, 1200)
    act(() => fire!())
    await frame()
    expect(document.querySelector('[data-testid="log-stub"]')!.isSameNode(log0), 'log form remounted on 3→2').toBe(true)
    expect(document.querySelector('[data-pane="decode"]')!.isSameNode(decode0), 'decode transcript remounted on 3→2').toBe(true)
  })

  it('collapses to ONE column when only the log can render (no empty 1fr band)', async () => {
    // Every CW pane except the log is ⊞-removable; with all of them hidden and no rig
    // capability, a 2-col template renders an EMPTY leading minmax(0,1fr) track beside
    // the log — the full-height "band of empty black" the model claims is
    // unrepresentable. The tier must collapse to 1 (region scroller; auto rows, so an
    // empty column takes no space).
    await renderCockpit({
      snap: makeSnap({ nb: null, nr: null, notch: null, nrLevel: null, agc: null }),
      onWorkSpot: undefined,
      panels: fakePanels(['decode', 'sent', 'copilot', 'bandActivity']),
    })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('1')
  })

  it('maxCols caps the tier at 2 when a column would sit empty', async () => {
    // No aux pane can render: the rig reports no DSP capability, no spots wire, and the
    // copilot is hidden from the ⊞ menu. A 3-track template with an empty middle is the
    // "band of empty black" rebuilt.
    await renderCockpit({
      snap: makeSnap({ nb: null, nr: null, notch: null, nrLevel: null, agc: null }),
      onWorkSpot: undefined,
      panels: fakePanels(['copilot', 'bandActivity']),
    })
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1800)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    expect(region.querySelectorAll(':scope > .cockpit-col').length).toBe(2)
    cleanup()

    // Mirror case: the main column is empty (DECODE hidden, nothing sent yet) while aux
    // panes exist — the leading track would be the empty one, so the tier caps likewise.
    await renderCockpit({ panels: fakePanels(['decode', 'sent']) })
    const region2 = document.querySelector('.cockpit-panes')!
    stubWidth(region2, 1800)
    act(() => fire!())
    await frame()
    expect(region2.getAttribute('data-cols')).toBe('2')
  })
})
