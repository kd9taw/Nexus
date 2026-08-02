// @vitest-environment jsdom
//
// RTTY COCKPIT SHELL STRUCTURE (2026-07-30 layout assessment, design3 §4 Phase 3).
//
// RTTY is the SMALL case of the pane-grid contract and the one place it is deliberately
// applied in part: the cockpit has exactly ONE operator-content block (the decoded
// stream), so it adopts CockpitPaneFrame + the pinned .cockpit-txdock and introduces NO
// pane region. A region would have to pick a tier, and every multi-column tier is a
// 2-or-3-track template with one filled column — i.e. it would MANUFACTURE the band of
// dead space this whole rebuild exists to delete. (At one column it is no better: the
// region's rows are content-height there, so the single frame would sit at its content
// height with a void below it — literally the RTTY bug fixed at styles.css ~16323.)
// The shell's own deficit valve (Batch 1) plus a grower frame is the whole mechanism here.
//
// What the tests pin: the stream renders through a frame; every transmit control (macros,
// the auto-sequencer row, Stop, the compose bar) renders in the dock and never inside a
// pane; the ⊞ menu still hides exactly the stream; and no region is introduced.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, RttyState } from '../types'
import type { PanelLayoutApi, RttyPanelId } from '../features/panelState'

const state: { current: RttyState } = {
  current: {
    armed: true,
    afcHz: 0,
    afcLocked: false,
    text: 'CQ CQ DE KD9TAW',
    charConf: [],
    baud: 45.45,
    shiftHz: 170,
    markHz: 2125,
    spaceHz: 2295,
    sending: false,
    backend: 'afsk',
    keyerError: null,
    auto: false,
    seqState: 'idle',
    peer: null,
    peerExchange: [],
    heardCq: null,
  } as unknown as RttyState,
}

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(async () => state.current),
  rttySend: vi.fn(async () => state.current),
  rttyStop: vi.fn(async () => state.current),
  rttyClear: vi.fn(async () => state.current),
  rttyAfcReset: vi.fn(async () => state.current),
  rttyNet: vi.fn(async () => state.current),
  rttySetAuto: vi.fn(async () => state.current),
  rttyAutoCq: vi.fn(async () => state.current),
  rttyAutoAnswer: vi.fn(async () => state.current),
  rttyAutoAbort: vi.fn(async () => state.current),
  haltTx: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts RTTY runs the waterfall at the
  // live-instrument 50 ms cadence, not the FT surfaces' 120 ms default.
  Waterfall: (p: { rowMs?: number }) => <div className="waterfall-wrap" data-rowms={p.rowMs} />,
}))

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.08,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
  },
} as unknown as AppSnapshot

function fakePanels(removed: RttyPanelId[] = []): PanelLayoutApi<RttyPanelId> {
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

async function renderCockpit(props: Partial<Parameters<typeof RttyCockpit>[0]> = {}) {
  const r = render(<RttyCockpit snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  state.current = { ...state.current, auto: false, seqState: 'idle', sending: false, keyerError: null }
})
afterEach(cleanup)

describe('RttyCockpit pane shell', () => {
  it('the shell holds no child kinds beyond the contract (design3 §5 rule 1)', async () => {
    // RTTY's sanctioned kinds: header chrome, the waterfall, the keyer-error banner,
    // the ONE shell-owned content frame, and the TX dock. Anything else is a new
    // shell-level sibling and must update this census deliberately.
    state.current = { ...state.current, keyerError: 'no FSK port' }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.rtty-cockpit')!
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.cw-keyer-warn', '.pane-frame', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(document.querySelector('.cw-keyer-warn'), 'warn banner did not render — census untested').not.toBeNull()
    expect(shell.querySelectorAll(':scope > .pane-frame').length).toBe(1)
    expect(shell.querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
  })

  it('the waterfall polls at the live-instrument cadence (50 ms), not the FT default', async () => {
    // RTTY is a live band instrument like the rig scope (PhoneScope, 20 Hz) — the producer
    // makes a fresh row every 20 ms, so the FT surfaces' 120 ms poll would discard 5 of every
    // 6 rows and cap the scroll at 8 rows/s (the operator's "smoothed out" report, 2026-07-30).
    await renderCockpit()
    expect(document.querySelector('.waterfall-wrap')!.getAttribute('data-rowms')).toBe('50')
  })

  it('the decoded stream renders through a CockpitPaneFrame', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="stream"]')
    expect(pane, 'the stream is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    // Its content — head controls and the transcript — is INSIDE the frame body.
    expect(pane!.querySelector('.cw-decode-text')).not.toBeNull()
    expect(pane!.querySelector('.rtty-arm')).not.toBeNull()
  })

  it('introduces no pane region (one content pane cannot fill a multi-track template)', async () => {
    await renderCockpit()
    expect(
      document.querySelectorAll('.cockpit-panes').length,
      'RTTY grew a pane region: with a single content block every tier leaves a track (or ' +
        'the space under a content-height row) empty — the dead space this rebuild removes.',
    ).toBe(0)
    expect(document.querySelectorAll('.cockpit-col').length).toBe(0)
  })

  it('every transmit control lives in the pinned TX dock — never inside a pane', async () => {
    state.current = { ...state.current, auto: true }
    await renderCockpit()
    const dock = document.querySelector('.cockpit-txdock')
    expect(dock, 'no .cockpit-txdock').not.toBeNull()
    for (const sel of [
      '.rtty-macros', // F-key macros + their-call + Stop
      '.rtty-auto-row', // auto-sequencer CQ / Answer / Abort
      '.rtty-stop', // the abort button specifically
      '.cw-send', // compose bar
      '.cw-type',
      '.cw-send-btn',
      '.rtty-hiscall',
    ]) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.cockpit-txdock'), `${sel} is not in the TX dock`).not.toBeNull()
      expect(
        el!.closest('.pane-frame'),
        `${sel} is inside a pane frame — a pane scrolls, transmit controls must not`,
      ).toBeNull()
    }
    expect(dock!.querySelector('.pane-frame')).toBeNull()
    // The dock comes AFTER the stream pane in the DOM (pinned at the bottom).
    const pane = document.querySelector('[data-pane="stream"]')!
    expect(pane.compareDocumentPosition(dock!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it("⊞ Panels 'removed' hides the stream pane and nothing else", async () => {
    await renderCockpit({ panels: fakePanels(['stream']) })
    expect(document.querySelector('[data-pane="stream"]')).toBeNull()
    // Stop and Send are not gated by the menu — they have no id to gate.
    expect(document.querySelector('.cockpit-txdock .rtty-stop')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
  })

  it('survives the keep-alive hide/show round trip with its structure intact', async () => {
    // The host renders RTTY permanently and toggles [hidden] (.rtty-host, styles.css
    // ~16240), so the cockpit unmounts nothing on navigation. Re-activation must not
    // need a remount to be structurally whole.
    const { rerender } = await renderCockpit({ active: true })
    await act(async () => {
      rerender(<RttyCockpit snap={snap} active={false} />)
    })
    await act(async () => {
      rerender(<RttyCockpit snap={snap} active={true} />)
    })
    expect(document.querySelector('[data-pane="stream"]')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
  })
})
