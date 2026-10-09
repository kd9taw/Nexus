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
import type { Ref } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, RttyState } from '../types'
import type { PanelLayoutApi, RttyPanelId } from '../features/panelState'
import { RTTY_PANELS, panelStorageKey, seamShares, usePanelLayout } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "the RF scope pane ships hidden; ticked, it is one more…", takes
// 0.39 s and 0.20 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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
// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// these cases check.
vi.mock('./PhoneScope', () => ({ PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-stub" data-feed={p.feed} /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts RTTY runs the waterfall at the
  // live-instrument 50 ms cadence, not the FT surfaces' 120 ms default.
  // …and forward `stripRef`: the strip's box reaches its divider through it (layout L6).
  Waterfall: (p: { rowMs?: number; stripRef?: Ref<HTMLDivElement> }) => (
    <div className="waterfall-wrap" data-rowms={p.rowMs} ref={p.stripRef} />
  ),
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
  it('the RF scope pane ships hidden; ticked, it is one more frame, FIRST under the TX strip, so the strip never moves', async () => {
    // Stock (no record): exactly the frames this census has always counted.
    await renderCockpit()
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    cleanup()
    // Ticked (this fixture docks every id): a frame like the transcript's, between the TX strip and
    // the transcript — the frames come after the strip, so a pane can never push the strip down.
    await renderCockpit({ panels: fakePanels() })
    const shell = document.querySelector('main.layout.single.rtty-cockpit')!
    const frames = Array.from(shell.querySelectorAll(':scope > .pane-frame')).map((f) => f.getAttribute('data-pane'))
    expect(frames).toEqual(['rfScope', 'stream', 'log'])
    const strip = shell.querySelector(':scope > .cockpit-txstrip')!
    expect(strip.previousElementSibling?.matches('.pane-splitter'), 'the TX strip left its place under the scope').toBe(true)
    expect(strip.nextElementSibling?.getAttribute('data-pane'), 'the RF scope pane is not first under the TX strip').toBe('rfScope')
    const pane = shell.querySelector('[data-pane="rfScope"]')!
    expect(pane.getAttribute('data-fit'), 'a scope can use the height it is given: a fill frame').toBe('fill')
    expect(pane.querySelector('[data-testid="rfscope-stub"]')?.getAttribute('data-feed')).toBe('rf')
    // Nothing in it transmits or stops: its one control is its own ✕.
    expect([...pane.querySelectorAll('button')].map((b) => b.getAttribute('aria-label'))).toEqual([expect.stringMatching(/RF scope/)])
  })

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
    // `.pane-splitter`: the divider between the transcript and the log strip (layout L6), a shell
    // child between the two frames when there is a record to keep the split in — the census with a
    // record is in the divider's own block below.
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.cw-keyer-warn', '.pane-frame', '.pane-splitter', '.cockpit-txstrip', '.cockpit-txdock']
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
    // THE TX STRIP (2026-10-01): exactly one, a shell child directly under the scope (after
    // its divider), holding the stop controls the header used to hold — and the header none.
    const strips = shell.querySelectorAll(':scope > .cockpit-txstrip')
    expect(strips.length, 'no TX strip in the shell').toBe(1)
    expect(strips[0].previousElementSibling?.matches('.pane-splitter'), 'the TX strip is not directly under the scope').toBe(true)
    const named = (root: Element, re: RegExp) => [...root.querySelectorAll('button')].filter((b) => re.test(b.textContent!.trim()))
    expect(named(strips[0], /^stop tx$/i).length, 'Stop TX is not in the TX strip').toBe(1)
    expect(named(strips[0], /^tune$/i).length, 'Tune is not in the TX strip').toBe(1)
    expect(named(shell.querySelector('.cockpit-header')!, /^stop tx$|^tune$|^tuning…$|^atu$|tx (on|off)$/i), 'the header still draws a transmit control').toEqual([])
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
      '.rtty-macros', // F1–F8 (their-call, TX and Stop sit in .rtty-dock-row beside or below)
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

// ── THE DIVIDER BETWEEN THE TRANSCRIPT AND THE LOG STRIP (layout L6) ─────────────────────────
// With the REAL panel record. jsdom lays nothing out, so a key's step is taken from stubbed boxes;
// the real layout (the pair's floors following the split in a short window) is measured in Chrome.
describe('RTTY: the divider between the transcript and the log strip', () => {
  let live: PanelLayoutApi<RttyPanelId> | null = null
  function Live() {
    const panels = usePanelLayout(RTTY_PANELS)
    live = panels
    return <RttyCockpit snap={snap} panels={panels} />
  }
  async function mountLive() {
    const r = render(<Live />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    return r
  }
  const pane = (id: string) => document.querySelector<HTMLElement>(`main.rtty-cockpit > [data-pane="${id}"]`)!
  const divider = () => screen.queryByRole('separator', { name: 'Decoded text / Log' })
  const stored = () => JSON.parse(localStorage.getItem(panelStorageKey('rtty')) ?? '{"share":{}}')
  const box = (el: HTMLElement, top: number, h: number) => {
    el.getBoundingClientRect = () => ({ top, bottom: top + h, height: h, left: 0, right: 900, width: 900, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
  }
  beforeEach(() => localStorage.clear())

  it('sits between the two frames, a shell child, only while both render and a record keeps the split', async () => {
    await mountLive()
    const sep = divider()
    expect(sep, 'no divider between the transcript and the log').not.toBeNull()
    expect(sep!.previousElementSibling).toBe(pane('stream'))
    expect(sep!.nextElementSibling).toBe(pane('log'))
    expect(sep!.parentElement!.matches('main.rtty-cockpit')).toBe(true)
    expect(sep!.tabIndex).toBe(0)
    cleanup()
    await renderCockpit()
    expect(divider(), 'no record to keep the split in (the Remote observer): no divider').toBeNull()
  })

  it('panes nobody has divided are the stock panes: their weights as the grow, the stock floor', async () => {
    await mountLive()
    expect(pane('stream').style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('stream').getAttribute('style')).toContain('var(--pane-share, 1) 1 0')
    expect(pane('log').getAttribute('style')).toContain('var(--pane-share, 1.5) 1 0')
    expect(pane('log').style.minHeight).toBe('min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, 1.25) / 1.25), 100%)')
  })

  it('a key stores the transcript’s share only; the frames carry grows that keep the pair’s stock total', async () => {
    await mountLive()
    box(pane('stream'), 100, 200)
    box(pane('log'), 312, 300)
    fireEvent.keyDown(divider()!, { key: 'ArrowDown' })
    const [a, b] = seamShares(200 / 500 + 0.05)
    expect(stored().share, 'the log strip has no id: only the transcript’s share is stored').toEqual({ stream: a })
    expect(Number(pane('stream').style.getPropertyValue('--pane-share'))).toBeCloseTo(a * 1.25, 10)
    expect(Number(pane('log').style.getPropertyValue('--pane-share'))).toBeCloseTo(b * 1.25, 10)
    fireEvent.keyDown(divider()!, { key: 'Backspace' })
    expect(stored().share).toEqual({})
    expect(pane('log').style.getPropertyValue('--pane-share')).toBe('')
  })

  it('Undo takes back one divider move and Reset layout the whole split (what ⊞ Panels calls)', async () => {
    // The header — where ⊞ Panels lives — is stubbed in this suite, so the record's own undo and
    // reset are called: the menu's Undo and Reset buttons are exactly those two (RttyCockpit's
    // `onUndo={panels.undo}` / `onReset={panels.reset}`).
    await mountLive()
    box(pane('stream'), 100, 200)
    box(pane('log'), 312, 300)
    fireEvent.keyDown(divider()!, { key: 'ArrowDown' })
    const [first] = seamShares(200 / 500 + 0.05)
    box(pane('stream'), 100, 225)
    box(pane('log'), 337, 275)
    fireEvent.keyDown(divider()!, { key: 'ArrowDown' })
    expect(stored().share.stream).toBeCloseTo(seamShares(225 / 500 + 0.05)[0], 10)
    act(() => live!.undo())
    expect(stored().share, 'Undo did not take back the last move').toEqual({ stream: first })
    expect(Number(pane('stream').style.getPropertyValue('--pane-share'))).toBeCloseTo(first * 1.25, 10)
    act(() => live!.reset())
    expect(stored().share, 'Reset layout kept the split').toEqual({})
    expect(pane('stream').style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('log').style.getPropertyValue('--pane-share')).toBe('')
  })

  it('a stored split comes back on the next start, the log’s half derived from the transcript’s', async () => {
    localStorage.setItem(panelStorageKey('rtty'), JSON.stringify({ v: 2, state: {}, share: { stream: 0.6 } }))
    await mountLive()
    expect(Number(pane('stream').style.getPropertyValue('--pane-share'))).toBeCloseTo(0.75, 10)
    expect(Number(pane('log').style.getPropertyValue('--pane-share'))).toBeCloseTo(1.75, 10)
  })

  it('with the transcript hidden there is no divider, and the log strip is the stock pane whatever split is stored', async () => {
    localStorage.setItem(panelStorageKey('rtty'), JSON.stringify({ v: 2, state: { stream: 'removed' }, share: { stream: 0.6 } }))
    await mountLive()
    expect(divider()).toBeNull()
    expect(pane('log').style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('log').style.minHeight).toBe('var(--cockpit-fill-min, 0)')
  })
})

// ── THE WATERFALL'S DIVIDER (layout L6) ──────────────────────────────────────────────────────
// RTTY's waterfall was a fixed 22 % of the viewport with no way to size it. Now a strip divider
// under it, the scope dividers' kind: focusable, arrows/Home/End/Backspace, its height in CSS px,
// stored per surface and clamped on load. jsdom lays nothing out, so the shell's box is stubbed;
// the clamps are the sheet's (WATERFALL_SPLIT_MIN/MAX) at jsdom's 16 px font and 768 px window.
describe('the RTTY waterfall divider', () => {
  function layOut(height: number) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('main.rtty-cockpit')) {
        return { top: 0, left: 0, width: 800, height, right: 800, bottom: height, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
      }
      return real.call(this)
    })
  }
  const shell = () => document.querySelector<HTMLElement>('main.rtty-cockpit')!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  const KEY = 'nexus.split.rtty.waterfall'
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--vh-eff')
    document.documentElement.style.removeProperty('--ui-zoom')
  })
  afterEach(() => vi.restoreAllMocks())

  it('sits under the waterfall, focusable, announcing its height, and steps, jumps and resets', async () => {
    layOut(1000)
    await renderCockpit()
    const sep = screen.getByRole('separator', { name: 'waterfall height' })
    expect(sep.previousElementSibling?.classList.contains('waterfall-wrap'), 'the divider is not under the strip').toBe(true)
    expect(sep.tabIndex).toBe(0)
    // 25 % of the shell; 8em at 16 px (under 28 % of the window); 45 % of the 768 px window.
    expect(aria(sep)).toEqual(['250', '128', '346'])
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(shell().style.getPropertyValue('--rtty-wf-h')).toBe(`${(266 / 1000) * 100}%`)
    expect(localStorage.getItem(KEY)).toBe(String((266 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'Home' })
    expect(sep.getAttribute('aria-valuenow')).toBe('128')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(shell().style.getPropertyValue('--rtty-wf-h')).toBe('25%')
    expect(localStorage.getItem(KEY)).toBe('25')
  })

  it('a stored height is restored, clamped against this window, and kept for a bigger one', async () => {
    localStorage.setItem(KEY, '75')
    layOut(1000)
    await renderCockpit()
    expect(screen.getByRole('separator', { name: 'waterfall height' }).getAttribute('aria-valuenow')).toBe('346')
    expect(localStorage.getItem(KEY), 'the clamp is apply-side only').toBe('75')
  })

  it('goes with the waterfall when the operator hides it', async () => {
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(screen.queryByRole('separator', { name: 'waterfall height' })).toBeNull()
  })
})
