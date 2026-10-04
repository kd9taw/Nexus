// @vitest-environment jsdom
//
// PSK COCKPIT SHELL STRUCTURE (Keyboard Modes Phase 2 — RX + TX).
//
// PSK follows RTTY's region-less shape of the pane-grid contract: one operator-content
// block (the decoded stream) through CockpitPaneFrame, deliberately NO pane region, and
// — since Phase 2 — the pinned .cockpit-txdock hosting every transmit control (macros,
// the continuous-TX latch, Esc/Stop, the compose bar, the ALC drive hint). THE STOP LINE
// is no longer held "by construction" (the Phase 1 no-TX census is gone, as that census's
// own header demanded): it is held the way RTTY holds it — Stop TX + the TX-enable latch
// in the header, the Esc/Stop macro in the dock, none with a ⊞ id — and the WIRING sweep
// for that lives in stop-line.test.tsx's PSK case. What THIS file pins is the shell
// census, the dock placement (transmit controls never inside a pane), and the view-entry
// auto-arm wiring (engine-owned policy, called once per activation edge).
import type { ReactNode, Ref } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { PskCockpit } from './PskCockpit'
import * as api from '../api'
import type { AppSnapshot, PskState } from '../types'
import type { PanelLayoutApi, PskPanelId } from '../features/panelState'
import { PSK_PANELS, panelStorageKey, seamShares, usePanelLayout } from '../features/panelState'

const state: { current: PskState } = {
  current: {
    armed: true,
    afcHz: 0,
    signal: false,
    centerHz: 1000,
    text: 'CQ CQ de KD9TAW',
    charConf: [],
    sending: false,
    latched: false,
    keyerError: null,
  },
}

vi.mock('../api', () => ({
  getPskState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  pskArm: vi.fn(async () => state.current),
  pskAutoArm: vi.fn(async () => state.current),
  pskClear: vi.fn(async () => state.current),
  pskAfcReset: vi.fn(async () => state.current),
  pskNet: vi.fn(async () => state.current),
  pskSend: vi.fn(async () => state.current),
  pskSetLatched: vi.fn(async () => state.current),
  pskSetMode: vi.fn(async () => state.current),
  pskType: vi.fn(async () => state.current),
  pskStop: vi.fn(async () => state.current),
  haltTx: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The header stub RENDERS its modeIndicator: the sub-mode selector + the QPSK
// Rev toggle live there (selector-adjacent), and the Phase 3 tests below pin
// them. Everything else about the header stays stubbed out.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => (
    <header className="cockpit-header">{p.modeIndicator}</header>
  ),
}))
// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// these cases check.
vi.mock('./PhoneScope', () => ({ PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-stub" data-feed={p.feed} /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts PSK runs the waterfall
  // at the live-instrument 50 ms cadence (the RTTY value), not the FT default.
  // …and forward `stripRef`: the strip's box reaches its divider through it (layout L6).
  Waterfall: (p: { rowMs?: number; stripRef?: Ref<HTMLDivElement> }) => (
    <div className="waterfall-wrap" data-rowms={p.rowMs} ref={p.stripRef} />
  ),
}))
// The log strip is stubbed for the same reason Phone's and CW's are in their own structure
// suites: this file is a SHELL census, and the real LogEntry reaches the logbook, the park
// directory and the callbook on mount. That the strip is actually WIRED — the right ADIF
// mode, the right default report, reachable from this cockpit at all — is PskCockpit.log.test.tsx.
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const pskAutoArm = api.pskAutoArm as ReturnType<typeof vi.fn>
const pskArm = api.pskArm as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.07,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
  },
} as unknown as AppSnapshot

function fakePanels(removed: PskPanelId[] = []): PanelLayoutApi<PskPanelId> {
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

async function renderCockpit(props: Partial<Parameters<typeof PskCockpit>[0]> = {}) {
  const r = render(<PskCockpit snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  state.current = {
    ...state.current,
    armed: true,
    signal: false,
    sending: false,
    latched: false,
    keyerError: null,
  }
  pskAutoArm.mockClear()
  pskArm.mockClear()
})
afterEach(cleanup)

describe('PskCockpit pane shell', () => {
  it('the RF scope pane ships hidden; ticked, it is one more frame, FIRST under the TX strip, so the strip never moves', async () => {
    // Stock (no record): exactly the frames this census has always counted.
    await renderCockpit()
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    cleanup()
    // Ticked (this fixture docks every id): a frame like the transcript's, between the TX strip and
    // the transcript — the frames come after the strip, so a pane can never push the strip down.
    await renderCockpit({ panels: fakePanels() })
    const shell = document.querySelector('main.layout.single.psk-cockpit')!
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

  it('the shell holds no child kinds beyond the census', async () => {
    // PSK's sanctioned kinds since Phase 2: header chrome, the waterfall, the
    // keyer-error banner, the content frames, and the TX dock — RTTY's
    // census exactly. A new shell-level sibling updates this deliberately or
    // does not ship.
    //
    // ⚠️ TWO CONTENT FRAMES SINCE #159 ("PSK has no LOG button"), and this is that
    // deliberate update. The second is the LOG strip — the CW/Phone `LogEntry`, framed like
    // theirs — and it is the SSTV shape rather than a new one: that shell has carried two
    // bare CockpitPaneFrames as direct children since its `-lower` region was deleted. It
    // stays out of the ⊞ vocabulary (PSK's is {scope, stream}, so the frame gets no ✕) and
    // it hosts no stop control; the dock assertions below are what hold that half.
    state.current = { ...state.current, keyerError: 'the rig didn’t accept PTT' }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.psk-cockpit')!
    expect(shell).not.toBeNull()
    // `.pane-splitter`: the transcript | log strip divider (layout L6), with a record — see its block.
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
    await renderCockpit()
    expect(document.querySelector('.waterfall-wrap')!.getAttribute('data-rowms')).toBe('50')
  })

  it('the decoded stream renders through a CockpitPaneFrame with its decoder controls', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="stream"]')
    expect(pane, 'the stream is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.querySelector('.cw-decode-text')).not.toBeNull()
    expect(pane!.querySelector('.rtty-arm')).not.toBeNull()
  })

  it('introduces no pane region (one content pane cannot fill a multi-track template)', async () => {
    await renderCockpit()
    expect(document.querySelectorAll('.cockpit-panes').length).toBe(0)
    expect(document.querySelectorAll('.cockpit-col').length).toBe(0)
  })

  it('every transmit control lives in the pinned TX dock — never inside a pane', async () => {
    await renderCockpit()
    const dock = document.querySelector('.cockpit-txdock')
    expect(dock, 'no .cockpit-txdock').not.toBeNull()
    for (const sel of [
      '.psk-macros', // F-key macros + their-call + TX latch + Stop
      '.psk-tx-latch', // the continuous-TX latch (a SENDER, not a stop)
      '.psk-stop', // the Esc/Stop abort specifically (on the stop-line census)
      '.cw-send', // compose bar
      '.cw-type',
      '.cw-send-btn',
      '.rtty-hiscall',
      '.psk-drive-hint', // the ALC/IMD drive sentence (the plan's dock hint)
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

  it('the Esc/Stop macro is live from the LATCH, not only from sending audio', async () => {
    // The RTTY rule, pinned on PSK's own dock: `sending` is stamped by the
    // radio loop from audio in flight, so it is false in the tick between the
    // latch going up and the first chunk keying — a census stop control that
    // is mounted and disabled then is the same loss as one that is gone.
    state.current = { ...state.current, sending: false, latched: false }
    await renderCockpit()
    let stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'idle: Stop has nothing to stop').toBe(true)
    cleanup()
    state.current = { ...state.current, sending: false, latched: true }
    await renderCockpit()
    stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'latched but not yet keying: Stop must already be live').toBe(false)
    cleanup()
    state.current = { ...state.current, sending: true, latched: false }
    await renderCockpit()
    stop = document.querySelector('.psk-stop') as HTMLButtonElement
    expect(stop.disabled, 'an over on the air: Stop is live').toBe(false)
  })

  it("⊞ 'Waterfall' unticked leaves the transcript to take the height, and vice versa", async () => {
    await renderCockpit({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.waterfall-wrap')).toBeNull()
    expect(document.querySelector('[data-pane="stream"]'), 'nothing left to take the freed height').not.toBeNull()
    cleanup()
    await renderCockpit({ panels: fakePanels(['stream']) })
    expect(document.querySelector('[data-pane="stream"]')).toBeNull()
    expect(document.querySelector('.waterfall-wrap'), 'the waterfall is a separate entry').not.toBeNull()
    // Stop and Send are not gated by the menu — they have no id to gate.
    expect(document.querySelector('.cockpit-txdock .psk-stop')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock .cw-send-btn')).not.toBeNull()
  })

  it('survives the keep-alive hide/show round trip with its structure intact', async () => {
    const { rerender } = await renderCockpit({ active: true })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={false} />)
    })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} />)
    })
    expect(document.querySelector('[data-pane="stream"]')).not.toBeNull()
    expect(document.querySelector('.cockpit-txdock')).not.toBeNull()
  })
})

describe('PSK view-entry auto-arm (the APRS/SSTV pattern, operator ruling 2026-08-17)', () => {
  it('entering the view calls the ENGINE policy exactly once per activation edge', async () => {
    const { rerender } = await renderCockpit({ active: true })
    expect(pskAutoArm).toHaveBeenCalledTimes(1)
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} theme="dark" />)
    })
    expect(pskAutoArm).toHaveBeenCalledTimes(1)
    // Leave and re-enter: a fresh edge, a fresh policy call (the engine decides
    // whether it arms — a session decline makes it a no-op THERE, not here).
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={false} />)
    })
    await act(async () => {
      rerender(<PskCockpit snap={snap} active={true} />)
    })
    expect(pskAutoArm).toHaveBeenCalledTimes(2)
  })

  it('never auto-arms while the view is hidden (the keep-alive host renders it inactive)', async () => {
    await renderCockpit({ active: false })
    expect(pskAutoArm).not.toHaveBeenCalled()
  })

  it("the pane's Arm button is the EXPLICIT stop/start the engine remembers", async () => {
    state.current = { ...state.current, armed: true }
    await renderCockpit()
    const btn = document.querySelector('.rtty-arm')! as HTMLButtonElement
    expect(btn.textContent).toBe('RX armed')
    await act(async () => {
      fireEvent.click(btn)
    })
    expect(pskArm).toHaveBeenCalledWith(false)
    // And starting again is the explicit arm that retires the decline (engine-side).
    state.current = { ...state.current, armed: false }
    cleanup()
    await renderCockpit()
    const btn2 = document.querySelector('.rtty-arm')! as HTMLButtonElement
    expect(btn2.textContent).toBe('Arm RX')
    await act(async () => {
      fireEvent.click(btn2)
    })
    expect(pskArm).toHaveBeenCalledWith(true)
  })
})

describe('PSK sub-mode selector + QPSK sideband reverse (Keyboard Modes Phase 3)', () => {
  const pskSetMode = api.pskSetMode as ReturnType<typeof vi.fn>

  it('the selector lists both modes, renders the engine truth, and PSK31 is the default', async () => {
    // `mode` absent from the poll (older engine / fresh state) reads as psk31.
    await renderCockpit()
    const sel = document.querySelector('.psk-mode-select') as HTMLSelectElement
    expect(sel, 'no sub-mode selector').not.toBeNull()
    expect(Array.from(sel.options).map((o) => o.value)).toEqual(['psk31', 'qpsk31'])
    expect(sel.value).toBe('psk31')
    cleanup()
    // The selector renders what the ENGINE says, not local state.
    state.current = { ...state.current, mode: 'qpsk31', reverse: false }
    await renderCockpit()
    const sel2 = document.querySelector('.psk-mode-select') as HTMLSelectElement
    expect(sel2.value).toBe('qpsk31')
  })

  it('changing the selector asks the ENGINE (which may refuse mid-transmission)', async () => {
    pskSetMode.mockClear()
    await renderCockpit()
    const sel = document.querySelector('.psk-mode-select') as HTMLSelectElement
    await act(async () => {
      fireEvent.change(sel, { target: { value: 'qpsk31' } })
    })
    expect(pskSetMode).toHaveBeenCalledWith('qpsk31', false)
  })

  it('the Rev toggle is selector-adjacent, QPSK-only, and flips the polarity through the engine', async () => {
    // BPSK is polarity-insensitive — no dead toggle there.
    state.current = { ...state.current, mode: 'psk31' }
    await renderCockpit()
    expect(document.querySelector('.psk-rev')).toBeNull()
    cleanup()
    state.current = { ...state.current, mode: 'qpsk31', reverse: false }
    await renderCockpit()
    const rev = document.querySelector('.psk-rev') as HTMLButtonElement
    expect(rev, 'no Rev toggle in QPSK31').not.toBeNull()
    // Selector-adjacent = inside the header's mode indicator, NOT a new TX
    // control in the dock and NOT inside any ⊞-removable pane.
    expect(rev.closest('.cockpit-header')).not.toBeNull()
    expect(rev.closest('.pane-frame')).toBeNull()
    expect(rev.getAttribute('aria-pressed')).toBe('false')
    pskSetMode.mockClear()
    await act(async () => {
      fireEvent.click(rev)
    })
    expect(pskSetMode).toHaveBeenCalledWith('qpsk31', true)
    cleanup()
    state.current = { ...state.current, mode: 'qpsk31', reverse: true }
    await renderCockpit()
    const rev2 = document.querySelector('.psk-rev') as HTMLButtonElement
    expect(rev2.getAttribute('aria-pressed')).toBe('true')
    pskSetMode.mockClear()
    await act(async () => {
      fireEvent.click(rev2)
    })
    expect(pskSetMode).toHaveBeenCalledWith('qpsk31', false)
  })

  it('QPSK31 adds no shell-level child and no TX-dock control (the stop-line census holds)', async () => {
    state.current = { ...state.current, mode: 'qpsk31', reverse: false }
    await renderCockpit()
    const shell = document.querySelector('main.layout.single.psk-cockpit')!
    // `.pane-splitter`: the transcript | log strip divider (layout L6), with a record — see its block.
    const ALLOWED = ['.cockpit-header', '.waterfall-wrap', '.cw-keyer-warn', '.pane-frame', '.pane-splitter', '.cockpit-txstrip', '.cockpit-txdock']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(document.querySelector('.cockpit-txdock .psk-rev')).toBeNull()
    expect(document.querySelector('.cockpit-txdock .psk-mode-select')).toBeNull()
  })
})

// ── THE DIVIDER BETWEEN THE TRANSCRIPT AND THE LOG STRIP (layout L6) ─────────────────────────
// RTTY's, for the same shell — with the REAL panel record; boxes stubbed (jsdom lays nothing out).
describe('PSK: the divider between the transcript and the log strip', () => {
  let live: PanelLayoutApi<PskPanelId> | null = null
  function Live() {
    const panels = usePanelLayout(PSK_PANELS)
    live = panels
    return <PskCockpit snap={snap} panels={panels} />
  }
  async function mountLive() {
    const r = render(<Live />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    return r
  }
  const pane = (id: string) => document.querySelector<HTMLElement>(`main.psk-cockpit > [data-pane="${id}"]`)!
  const divider = () => screen.queryByRole('separator', { name: 'Decoded text / Log' })
  const stored = () => JSON.parse(localStorage.getItem(panelStorageKey('psk')) ?? '{"share":{}}')
  const box = (el: HTMLElement, top: number, h: number) => {
    el.getBoundingClientRect = () => ({ top, bottom: top + h, height: h, left: 0, right: 900, width: 900, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
  }
  beforeEach(() => localStorage.clear())

  it('sits between the two frames only while both render and a record keeps the split', async () => {
    await mountLive()
    const sep = divider()
    expect(sep, 'no divider between the transcript and the log').not.toBeNull()
    expect(sep!.previousElementSibling).toBe(pane('stream'))
    expect(sep!.nextElementSibling).toBe(pane('log'))
    cleanup()
    await renderCockpit()
    expect(divider(), 'no record (the Remote observer): no divider').toBeNull()
  })

  it('stock panes are untouched; a key stores the transcript’s share and the frames carry the grows', async () => {
    await mountLive()
    expect(pane('stream').style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('log').getAttribute('style')).toContain('var(--pane-share, 1.5) 1 0')
    box(pane('stream'), 100, 200)
    box(pane('log'), 312, 300)
    fireEvent.keyDown(divider()!, { key: 'ArrowUp' })
    const [a, b] = seamShares(200 / 500 - 0.05)
    expect(stored().share).toEqual({ stream: a })
    expect(Number(pane('stream').style.getPropertyValue('--pane-share'))).toBeCloseTo(a * 1.25, 10)
    expect(Number(pane('log').style.getPropertyValue('--pane-share'))).toBeCloseTo(b * 1.25, 10)
  })

  it('Undo takes back one divider move and Reset layout the whole split (what ⊞ Panels calls)', async () => {
    // The header — where ⊞ Panels lives — is stubbed in this suite, so the record's own undo and
    // reset are called: the menu's Undo and Reset buttons are exactly those two (PskCockpit's
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

  it('with the transcript hidden there is no divider and the log strip is the stock pane', async () => {
    localStorage.setItem(panelStorageKey('psk'), JSON.stringify({ v: 2, state: { stream: 'removed' }, share: { stream: 1.7 } }))
    await mountLive()
    expect(divider()).toBeNull()
    expect(pane('log').style.minHeight).toBe('var(--cockpit-fill-min, 0)')
  })
})

// ── THE WATERFALL'S DIVIDER (layout L6) ──────────────────────────────────────────────────────
// PSK's waterfall was a fixed 22 % of the viewport with no way to size it. Now a strip divider
// under it, the scope dividers' kind: focusable, arrows/Home/End/Backspace, its height in CSS px,
// stored per surface and clamped on load. jsdom lays nothing out, so the shell's box is stubbed;
// the clamps are the sheet's (WATERFALL_SPLIT_MIN/MAX) at jsdom's 16 px font and 768 px window.
describe('the PSK waterfall divider', () => {
  function layOut(height: number) {
    const real = HTMLElement.prototype.getBoundingClientRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('main.psk-cockpit')) {
        return { top: 0, left: 0, width: 800, height, right: 800, bottom: height, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
      }
      return real.call(this)
    })
  }
  const shell = () => document.querySelector<HTMLElement>('main.psk-cockpit')!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  const KEY = 'nexus.split.psk.waterfall'
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
