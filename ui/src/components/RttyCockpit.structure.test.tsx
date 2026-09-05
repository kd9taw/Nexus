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
    latched: false,
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
  // `rtty_auto_arm` fires on the rising edge of `active`; a hand-kept mock must carry it or
  // the cockpit throws on mount. Wave-2 backend contract — the UI half is what is exercised here.
  rttyAutoArm: vi.fn(async () => state.current),
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
// The log strip is stubbed for the same reason PSK's, Phone's and CW's are in their own
// structure suites: this file is a SHELL census, and the real LogEntry reaches the logbook,
// the park directory and the callbook on mount. That the strip is actually WIRED — the right
// ADIF mode, the Field Day class/section route — is RttyCockpit.log.test.tsx.
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
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
    undoRemoves: [],
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
    // the shell-owned content frames, and the TX dock. Anything else is a new
    // shell-level sibling and must update this census deliberately.
    //
    // ⚠️ TWO CONTENT FRAMES SINCE THE FIELD DAY LOGGING FIX, and this is that deliberate
    // update — PSK's, word for word (its own census note carries the same paragraph). The
    // second is the LOG strip: this cockpit rendered no LogEntry at all, so a station worked
    // by hand had nowhere to go, and on Field Day — which is all-mode, RTTY included — there
    // was no way to enter the class/section exchange that scores. It stays out of the ⊞
    // vocabulary (RTTY's is {stream}, so the frame gets no ✕) and it hosts no stop control;
    // the dock assertions below are what hold that half.
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
    // Named, not just counted: a count alone goes green on the stream frame rendering twice.
    const frames = Array.from(shell.querySelectorAll(':scope > .pane-frame'))
    expect(frames.map((f) => f.getAttribute('data-pane'))).toEqual(['stream', 'log'])
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
    // The waterfall is a SEPARATE entry and must not go with it (2026-08-16).
    expect(document.querySelector('.waterfall-wrap')).not.toBeNull()
    // Stop and Send are not gated by the menu — they have no id to gate.
    expect(document.querySelector('.cockpit-txdock .rtty-stop')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
  })

  // ── THE WATERFALL IS A PANEL NOW (operator, 2026-08-16) ────────────────────────────
  it('the waterfall is SHOWN by default — the ⊞ entry is an option, not a new default', async () => {
    await renderCockpit()
    expect(document.querySelector('.waterfall-wrap'), 'the waterfall went missing on a stock layout').not.toBeNull()
  })

  it("⊞ 'Waterfall' unticked leaves the transcript to take the height", async () => {
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.waterfall-wrap'), 'the waterfall survived its own hide').toBeNull()
    // `.rtty-cockpit > .pane-frame` is this shell's only grower, so the 22%-of-viewport strip
    // the tick freed goes to the transcript. It has to still be there to receive it.
    expect(document.querySelector('[data-pane="stream"]'), 'nothing is left to take the freed height').not.toBeNull()
  })

  it('both ids unticked leaves the header and dock stop controls untouched', async () => {
    // RTTY's whole ⊞ vocabulary is these two, so this is the cockpit where "hide everything"
    // is closest to hiding the screen. THE STOP LINE says what must survive it, and the
    // wiring half is computed in stop-line.test.tsx against the REAL header; this asserts the
    // shell-level half — that neither id reaches the dock.
    await renderCockpit({ panels: fakePanels(['scope', 'stream']) })
    expect(document.querySelector('.waterfall-wrap')).toBeNull()
    expect(document.querySelector('[data-pane="stream"]')).toBeNull()
    expect(document.querySelector('.cockpit-txdock .rtty-stop'), 'the dock Stop went with the panes').not.toBeNull()
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
