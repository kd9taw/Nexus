// @vitest-environment jsdom
//
// SSTV SHELL STRUCTURE (2026-07-30 layout assessment; census growers.md #9/#10).
//
// SSTV follows RTTY's shape of the pane-grid contract: CockpitPaneFrame for the
// operator-content blocks, deliberately NO .cockpit-panes region — with two content
// blocks every multi-column tier is a 2-or-3-track template with one filled column,
// i.e. manufactured dead space. The roles fix census #10 directly: the Transmit
// composer is a bounded strip (fit="content" — a drop zone cannot use surplus), the
// Gallery is the fill grower beside the RX stage. The `.sstv-lower` 50/50 wrapper
// (each pane took half the region whatever it held) is deleted, not restyled.
//
// The RX stage (.sstv-canvas) stays the shell's own bounded flex child, NOT a frame:
// it is canvas-centric (the live decode / the band waterfall), its sizing context is
// the shell's flex share (1.1) with the band's self-disarming floor, and a frame's
// .pane-body scroller would hand its height to content measurement instead.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup, fireEvent, screen } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'
import type { PanelLayoutApi, SstvPanelId } from '../features/panelState'

// THE BUDGET (2026-10-09). The slowest case here, "the RF scope pane ships hidden; ticked, it is one more…", takes
// 0.27 s and 0.19 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

// The RF scope pane's picture is a renderer canvas jsdom cannot draw; its frame and place are what
// these cases check.
vi.mock('./PhoneScope', () => ({ PhoneScope: (p: { feed?: string }) => <div data-testid="rfscope-stub" data-feed={p.feed} /> }))
vi.mock('./Waterfall', () => ({
  // Capture the cadence prop: the liveliness pin below asserts the SSTV band waterfall runs at
  // the live-instrument 50 ms cadence, not the FT surfaces' 120 ms default.
  Waterfall: (p: { rowMs?: number }) => <div data-testid="band-waterfall" data-rowms={p.rowMs} />,
}))
// The rotor strip has its own suite (SstvView.rotor.test.tsx), and it reaches the rotator on mount.
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>

const snap = {
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const NO_HEALTH: SstvHealth = {
  armed: false,
  audioPeak: 0,
  lastAudioUnix: null,
  drains: 0,
  visSeen: 0,
  lastVisUnix: null,
  unknownVis: 0,
  lastUnknownVisCode: null,
  lastUnknownVisUnix: null,
  images: 0,
  lastImageUnix: null,
}

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: NO_HEALTH,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

function fakePanels(removed: SstvPanelId[] = []): PanelLayoutApi<SstvPanelId> {
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

async function renderView(props: Partial<Parameters<typeof SstvView>[0]> = {}) {
  const r = render(<SstvView snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  // Entering the view starts the receiver (`sstv_auto_arm`). These cases are about
  // the shell's structure, so it just resolves to the same state.
  ;(api.sstvAutoArm as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue(IDLE)
})
afterEach(cleanup)

describe('SstvView pane shell', () => {
  it('the RF scope pane ships hidden; ticked, it is one more frame, FIRST under the TX strip, so the strip never moves', async () => {
    await renderView()
    expect(document.querySelector('[data-pane="rfScope"]'), 'the RF scope pane shipped visible').toBeNull()
    cleanup()
    await renderView({ panels: fakePanels() })
    const shell = document.querySelector('main.layout.single.sstv-view')!
    const frames = Array.from(shell.querySelectorAll(':scope > .pane-frame')).map((f) => f.getAttribute('data-pane'))
    expect(frames).toEqual(['rfScope', 'txcompose', 'gallery'])
    const strip = shell.querySelector(':scope > .cockpit-txstrip')!
    expect(strip.previousElementSibling?.matches('.pane-splitter'), 'the TX strip left its place under the stage').toBe(true)
    expect(strip.nextElementSibling?.getAttribute('data-pane'), 'the RF scope pane is not first under the TX strip').toBe('rfScope')
    const pane = shell.querySelector('[data-pane="rfScope"]')!
    expect(pane.getAttribute('data-fit')).toBe('fill')
    expect(pane.querySelector('[data-testid="rfscope-stub"]')?.getAttribute('data-feed')).toBe('rf')
    // The RX stage is untouched by it: the band (or a picture) stays the stage.
    expect(shell.querySelector(':scope > .sstv-canvas')).not.toBeNull()
  })

  it('the shell holds no child kinds beyond the census (header, RX stage and its divider, TX bar, frames)', async () => {
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    expect(shell).not.toBeNull()
    // The stage's divider (layout L6) is a shell child, as the scope dividers are in Phone/CW/JS8.
    const ALLOWED = ['.cockpit-header', '.sstv-canvas', '.pane-splitter', '.cockpit-txstrip', '.sstv-tx-bar', '.pane-frame']
    for (const el of Array.from(shell.children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell.querySelectorAll(':scope > .pane-frame').length).toBe(2)
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

  it('the Transmit composer renders through a frame with fit="content" (a drop zone cannot stretch)', async () => {
    await renderView()
    const pane = document.querySelector('[data-pane="txcompose"]')
    expect(pane, 'the composer is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.getAttribute('data-fit')).toBe('content')
    // Its content is inside the frame body, not a shell-level sibling.
    expect(pane!.querySelector('.pane-body .sstv-tx-drop')).not.toBeNull()
    expect(pane!.querySelector('.pane-body input[type=file]')).not.toBeNull()
  })

  it('the Gallery renders through a fill frame (the one lower grower beside the RX stage)', async () => {
    await renderView()
    const pane = document.querySelector('[data-pane="gallery"]')
    expect(pane, 'the gallery is not framed').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.getAttribute('data-fit')).toBe('fill')
    expect(pane!.querySelector('.pane-body .sstv-gallery-grid')).not.toBeNull()
  })

  it('introduces no pane region and the .sstv-lower 50/50 wrapper is gone', async () => {
    await renderView()
    expect(
      document.querySelectorAll('.cockpit-panes').length,
      'SSTV grew a pane region: two content blocks cannot fill a multi-track template',
    ).toBe(0)
    expect(document.querySelectorAll('.cockpit-col').length).toBe(0)
    expect(
      document.querySelectorAll('.sstv-lower').length,
      '.sstv-lower is back — the wrapper whose 50/50 split is census finding #10',
    ).toBe(0)
  })

  it('the RX stage is the shell-owned bounded stage, never framed', async () => {
    await renderView()
    const stage = document.querySelector('.sstv-canvas')
    expect(stage).not.toBeNull()
    expect(stage!.closest('.pane-frame')).toBeNull()
  })

  // ── THE BAND WATERFALL IS A PANEL NOW (operator, 2026-08-16) ──────────────────────
  // "add the waterfall in each window as an option to remove in the panels section — leave
  // it ON by default, give me the option to turn it off." SSTV is the awkward one: the tick
  // reaches only HALF of a region that is the band until a VIS lands and the PICTURE after
  // it. Three cases, because the interesting one is the third.
  it('the band waterfall is SHOWN by default — the ⊞ entry is an option, not a new default', async () => {
    await renderView()
    expect(document.querySelector('[data-testid="band-waterfall"]')).not.toBeNull()
    expect(document.querySelector('.sstv-canvas')).not.toBeNull()
  })

  it("⊞ 'Waterfall' unticked drops the whole idle stage, not just the canvas inside it", async () => {
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('[data-testid="band-waterfall"]'), 'the waterfall survived its own hide').toBeNull()
    // AND the section with it. `.sstv-canvas` is `flex: 1.1 1 0` with a 16em floor: keep it
    // and empty it and the operator trades a waterfall for a 16em bordered void that hoards
    // the height he ticked the box to reclaim. This is the assertion that would catch the
    // tempting one-line version of this change (gating the <Waterfall> alone).
    expect(document.querySelector('.sstv-canvas'), 'an empty 16em stage is left hoarding the height').toBeNull()
    // The lower panes are still there to receive it — the gallery is the shell's fill grower.
    expect(document.querySelector('[data-pane="gallery"]')).not.toBeNull()
    expect(document.querySelector('[data-pane="txcompose"]')).not.toBeNull()
    // And the TX bar, which is where Stop lives, is untouched by any tick (THE STOP LINE).
    expect(document.querySelector('.sstv-tx-stop')).not.toBeNull()
  })

  it('a picture in flight takes the stage even with the waterfall unticked', async () => {
    // The tick hides the BAND, not the decode. An operator who unticked the waterfall last
    // week has not asked to stop seeing pictures — and this view exists to show them.
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      mode: 'Robot 36',
      linesDone: 1,
      linesTotal: 240,
      previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
      previewWidth: 2,
      previewHeight: 1,
    })
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-canvas'), 'the RX stage went with the band waterfall').not.toBeNull()
    expect(document.querySelector('.sstv-live'), 'no picture on the stage while one is decoding').not.toBeNull()
    expect(document.querySelector('[data-testid="band-waterfall"]'), 'the band is back under a picture').toBeNull()
  })

  it('a picture arriving onto a HIDDEN-waterfall stage is still measured and upscaled', async () => {
    // The stage is not permanent any more, and the measurement has to follow it. With the
    // waterfall unticked the section does not exist while idle and mounts the instant a VIS
    // lands — so a mount-only ResizeObserver would attach when there was nothing to observe
    // and never again, and the picture would fall back to the sheet's 480px 3× default with
    // the integer upscale silently never applied. Same fixture as the measurement case below,
    // one id hidden: 1000×800 stage, native 2×1, 6× cap ⇒ 12px.
    const cw = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
    const ch = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 1000 })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 800 })
    try {
      getSstvState.mockResolvedValue({
        ...IDLE,
        armed: true,
        mode: 'Robot 36',
        linesDone: 1,
        linesTotal: 240,
        previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
        previewWidth: 2,
        previewHeight: 1,
      })
      await renderView({ panels: fakePanels(['scope']) })
      const canvas = document.querySelector('.sstv-live-canvas') as HTMLElement
      expect(canvas, 'no live canvas rendered with the waterfall hidden').not.toBeNull()
      expect(
        canvas.style.getPropertyValue('--sstv-img-w'),
        'the stage was never measured — the picture fell back to the sheet default',
      ).toBe('12px')
    } finally {
      if (cw) Object.defineProperty(Element.prototype, 'clientWidth', cw)
      if (ch) Object.defineProperty(Element.prototype, 'clientHeight', ch)
    }
  })

  it('the band waterfall polls at the live-instrument cadence (50 ms), not the FT default', async () => {
    // The idle RX stage IS the band ("what's on the frequency right now") — a live instrument
    // like the rig scope, not a slot-synchronous FT surface. The producer makes a fresh row
    // every 20 ms; the FT 120 ms poll would discard 5 of 6 rows (operator report 2026-07-30).
    await renderView()
    const wf = document.querySelector('[data-testid="band-waterfall"]')
    expect(wf, 'idle band waterfall did not render — cadence untested').not.toBeNull()
    expect(wf!.getAttribute('data-rowms')).toBe('50')
  })

  it('the TX bar is the LAST shell child — parked in the deficit valve (dock discipline)', async () => {
    await renderView()
    const shell = document.querySelector('main.layout.single.sstv-view')!
    expect(
      shell.lastElementChild!.classList.contains('sstv-tx-bar'),
      'the bar must be the final child: sticky bottom parks it at the scrollport bottom, so ' +
        'when the shell valve scrolls, Send/Stop never leave reach. Mid-column, sticky ' +
        'bottom cannot stop Stop scrolling off the TOP on the way down to the gallery.',
    ).toBe(true)
  })

  it('TX controls (Send / Stop / mode) live in the pinned bar, never inside a pane', async () => {
    await renderView()
    for (const sel of ['.sstv-tx-send', '.sstv-tx-stop', '.sstv-tx-mode']) {
      const el = document.querySelector(sel)
      expect(el, `${sel} missing`).not.toBeNull()
      expect(el!.closest('.sstv-tx-bar'), `${sel} is not in the TX bar`).not.toBeNull()
      expect(
        el!.closest('.pane-frame'),
        `${sel} is inside a pane frame — a pane scrolls, transmit controls must not`,
      ).toBeNull()
    }
  })

  it("⊞ Panels 'removed' hides exactly the named pane; Send/Stop have no id to gate", async () => {
    await renderView({ panels: fakePanels(['txcompose']) })
    expect(document.querySelector('[data-pane="txcompose"]')).toBeNull()
    expect(document.querySelector('[data-pane="gallery"]')).not.toBeNull()
    expect(document.querySelector('.sstv-tx-send')).not.toBeNull()
    expect(document.querySelector('.sstv-tx-stop')).not.toBeNull()
  })

  it('stamps an integer-multiple --sstv-img-w on the live canvas from the measured stage', async () => {
    // jsdom reports clientWidth/Height 0 (the 0×0 keep-alive guard would keep the last
    // real size); give every element a fake layout so the stage measure sees 1000×800.
    // Decode native is 2×1 (the existing preview fixture): avail ≈ 968×736 allows far
    // more than the 6× cap ⇒ k = 6 ⇒ width 12px. Fractional would be e.g. 484px.
    const cw = Object.getOwnPropertyDescriptor(Element.prototype, 'clientWidth')
    const ch = Object.getOwnPropertyDescriptor(Element.prototype, 'clientHeight')
    Object.defineProperty(Element.prototype, 'clientWidth', { configurable: true, get: () => 1000 })
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => 800 })
    try {
      getSstvState.mockResolvedValue({
        ...IDLE,
        armed: true,
        mode: 'Robot 36',
        linesDone: 1,
        linesTotal: 240,
        previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
        previewWidth: 2,
        previewHeight: 1,
      })
      await renderView()
      const canvas = document.querySelector('.sstv-live-canvas') as HTMLElement
      expect(canvas, 'no live canvas rendered').not.toBeNull()
      expect(canvas.style.getPropertyValue('--sstv-img-w')).toBe('12px')
    } finally {
      if (cw) Object.defineProperty(Element.prototype, 'clientWidth', cw)
      if (ch) Object.defineProperty(Element.prototype, 'clientHeight', ch)
    }
  })
})

// ── THE STAGE'S DIVIDER (layout L6) ──────────────────────────────────────────────────────────
// The stage is the column's grower — the one shape that follows this view at every window, and
// the picture's integer upscale rides on it — so its divider paints NOTHING until the operator
// moves it; moved, the stage is marked `data-sized` (its sheet rule's key) and the size is stored
// per surface; a reset gives the grower back. jsdom lays nothing out, so the shell and the stage
// are stubbed: the stock stage stands at 300 px of a 1000 px shell, and the sized one lays out like
// its rule (the painted basis, the 16em floor, 70 % of the window) at jsdom's 16 px / 768 px.
describe('the SSTV stage divider', () => {
  function layOut() {
    const real = HTMLElement.prototype.getBoundingClientRect
    const rect = (height: number) =>
      ({ top: 0, left: 0, width: 800, height, right: 800, bottom: height, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('main.sstv-view')) return rect(1000)
      if (this.matches('.sstv-canvas')) {
        if (!this.hasAttribute('data-sized')) return rect(300)
        const v = shell().style.getPropertyValue('--sstv-stage-h')
        const basis = v === '' ? 350 : v.endsWith('%') ? parseFloat(v) * 10 : parseFloat(v)
        return rect(Math.min(0.7 * 768, Math.max(16 * 16, basis)))
      }
      return real.call(this)
    })
  }
  const shell = () => document.querySelector<HTMLElement>('main.sstv-view')!
  const stage = () => document.querySelector<HTMLElement>('.sstv-canvas')!
  const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => el.getAttribute(a))
  const divider = () => screen.queryByRole('separator', { name: 'waterfall and picture height' })
  const KEY = 'nexus.split.sstv.stage'
  const LIVE: SstvState = {
    ...IDLE,
    armed: true,
    mode: 'Robot 36',
    linesDone: 1,
    linesTotal: 240,
    previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
    previewWidth: 2,
    previewHeight: 1,
  }
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--vh-eff')
    document.documentElement.style.removeProperty('--ui-zoom')
  })
  afterEach(() => vi.restoreAllMocks())

  it('sits right under the stage, a shell child outside every pane, and the TX bar stays last', async () => {
    await renderView()
    const sep = divider()
    expect(sep, 'no divider under the stage').not.toBeNull()
    expect(sep!.previousElementSibling, 'the divider is not under the stage').toBe(stage())
    expect(sep!.parentElement).toBe(shell())
    expect(sep!.closest('.pane-frame')).toBeNull()
    expect(sep!.tabIndex).toBe(0)
    expect(shell().lastElementChild!.classList.contains('sstv-tx-bar')).toBe(true)
  })

  it('stock: the stage keeps the size the sheet gives it — nothing painted, marked or stored', async () => {
    layOut()
    await renderView()
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe('')
    expect(stage().hasAttribute('data-sized')).toBe(false)
    expect(localStorage.getItem(KEY)).toBeNull()
    // The stage as it stands; the 16em floor at 16 px; 70 % of the 768 px window.
    expect(aria(divider()!)).toEqual(['300', '256', '538'])
  })

  it('a key sizes the stage from where it stands, a reset gives the grower back', async () => {
    layOut()
    await renderView()
    const sep = divider()!
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe(`${(316 / 1000) * 100}%`)
    expect(stage().hasAttribute('data-sized')).toBe(true)
    expect(localStorage.getItem(KEY)).toBe(String((316 / 1000) * 100))
    fireEvent.keyDown(sep, { key: 'End' })
    expect(sep.getAttribute('aria-valuenow')).toBe('538')
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(shell().style.getPropertyValue('--sstv-stage-h')).toBe('')
    expect(stage().hasAttribute('data-sized')).toBe(false)
    expect(sep.getAttribute('aria-valuenow')).toBe('300')
  })

  it('a stored height is restored sized, clamped against this window, and kept for a bigger one', async () => {
    localStorage.setItem(KEY, '75')
    layOut()
    await renderView()
    expect(stage().hasAttribute('data-sized')).toBe(true)
    expect(divider()!.getAttribute('aria-valuenow')).toBe('538')
    expect(localStorage.getItem(KEY), 'the clamp is apply-side only').toBe('75')
  })

  it('goes with the stage when the waterfall is unticked and nothing is decoding', async () => {
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-canvas')).toBeNull()
    expect(divider()).toBeNull()
  })

  it('stays with the stage while a picture is in flight, the waterfall unticked or not', async () => {
    getSstvState.mockResolvedValue(LIVE)
    await renderView({ panels: fakePanels(['scope']) })
    expect(document.querySelector('.sstv-live'), 'no picture on the stage').not.toBeNull()
    expect(divider(), 'the picture lost its divider with the waterfall').not.toBeNull()
    expect(divider()!.previousElementSibling).toBe(stage())
  })
})

// (The computed-winner check that the sheet's `.sstv-live-canvas` width actually consumes
// the stamp lives in cockpit-shells.test.ts, beside the file's other cascade computers —
// this jsdom file cannot read styles.css: import.meta.url is not file-scheme here.)
